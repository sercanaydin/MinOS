//! Intel e1000 (82540EM) Ethernet sürücüsü — yoklama (polling) tabanlı.
//!
//! QEMU'nun `-device e1000` kartını sürer. Kesme kullanmaz; alım/gönderim
//! tamamlanmasını descriptor durum bitlerini yoklayarak izler. DMA tamponları
//! statiktir (sayfalama kapalı → adres = fiziksel adres).

#![allow(dead_code)]

use crate::serial;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

// --- Yazmaç ofsetleri ---
const REG_CTRL: usize = 0x0000;
const REG_STATUS: usize = 0x0008;
const REG_ICR: usize = 0x00C0;
const REG_IMC: usize = 0x00D8;
const REG_RCTL: usize = 0x0100;
const REG_TCTL: usize = 0x0400;
const REG_TIPG: usize = 0x0410;
const REG_RDBAL: usize = 0x2800;
const REG_RDBAH: usize = 0x2804;
const REG_RDLEN: usize = 0x2808;
const REG_RDH: usize = 0x2810;
const REG_RDT: usize = 0x2818;
const REG_TDBAL: usize = 0x3800;
const REG_TDBAH: usize = 0x3804;
const REG_TDLEN: usize = 0x3808;
const REG_TDH: usize = 0x3810;
const REG_TDT: usize = 0x3818;
const REG_RAL: usize = 0x5400;
const REG_RAH: usize = 0x5404;

const RCTL_EN: u32 = 1 << 1;
const RCTL_BAM: u32 = 1 << 15; // broadcast kabul
const RCTL_SECRC: u32 = 1 << 26; // CRC'yi at
// BSIZE bitleri (16-17)=00 + BSEX(25)=0 → 2048 bayt tampon.

const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;

// TX komut bitleri.
const TX_EOP: u8 = 1 << 0;
const TX_IFCS: u8 = 1 << 1;
const TX_RS: u8 = 1 << 3;
const TX_DD: u8 = 1 << 0; // status: descriptor done
const RX_DD: u8 = 1 << 0;
const RX_EOP: u8 = 1 << 1;

const NRX: usize = 32;
const NTX: usize = 8;
const BUFSZ: usize = 2048;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct RxDesc {
    addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct TxDesc {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

#[repr(C, align(16))]
struct RxRing([RxDesc; NRX]);
#[repr(C, align(16))]
struct TxRing([TxDesc; NTX]);
#[repr(C, align(16))]
struct RxBufs([[u8; BUFSZ]; NRX]);
#[repr(C, align(16))]
struct TxBufs([[u8; BUFSZ]; NTX]);

static mut RX: RxRing = RxRing(
    [RxDesc {
        addr: 0,
        length: 0,
        checksum: 0,
        status: 0,
        errors: 0,
        special: 0,
    }; NRX],
);
static mut TX: TxRing = TxRing(
    [TxDesc {
        addr: 0,
        length: 0,
        cso: 0,
        cmd: 0,
        status: 0,
        css: 0,
        special: 0,
    }; NTX],
);
static mut RXBUF: RxBufs = RxBufs([[0; BUFSZ]; NRX]);
static mut TXBUF: TxBufs = TxBufs([[0; BUFSZ]; NTX]);

struct E1000 {
    base: usize,
    mac: [u8; 6],
    rx_cur: usize,
    tx_cur: usize,
    ready: bool,
}

static mut DEV: E1000 = E1000 {
    base: 0,
    mac: [0; 6],
    rx_cur: 0,
    tx_cur: 0,
    ready: false,
};

#[inline]
unsafe fn reg_r(base: usize, off: usize) -> u32 {
    read_volatile((base + off) as *const u32)
}

#[inline]
unsafe fn reg_w(base: usize, off: usize, v: u32) {
    write_volatile((base + off) as *mut u32, v);
}

/// Kartı bulup başlatır. Başarılıysa `true`.
pub fn init() -> bool {
    let base = match crate::pci::find_e1000() {
        Some(b) => b as usize,
        None => return false,
    };
    let dev = addr_of_mut!(DEV);
    unsafe {
        (*dev).base = base;

        // Tüm kesmeleri maskele (yoklama yapıyoruz).
        reg_w(base, REG_IMC, 0xFFFF_FFFF);
        let _ = reg_r(base, REG_ICR);

        // MAC adresini RAL/RAH'tan oku (QEMU sıfırlama sonrası doldurur).
        let ral = reg_r(base, REG_RAL);
        let rah = reg_r(base, REG_RAH);
        (*dev).mac = [
            ral as u8,
            (ral >> 8) as u8,
            (ral >> 16) as u8,
            (ral >> 24) as u8,
            rah as u8,
            (rah >> 8) as u8,
        ];

        // RX halkası.
        let rx = addr_of_mut!(RX) as *mut RxDesc;
        for i in 0..NRX {
            let buf = addr_of!(RXBUF.0[i]) as u64;
            (*rx.add(i)).addr = buf;
            (*rx.add(i)).status = 0;
        }
        let rx_phys = addr_of!(RX) as u32;
        reg_w(base, REG_RDBAL, rx_phys);
        reg_w(base, REG_RDBAH, 0);
        reg_w(base, REG_RDLEN, (NRX * 16) as u32);
        reg_w(base, REG_RDH, 0);
        reg_w(base, REG_RDT, (NRX - 1) as u32);
        reg_w(base, REG_RCTL, RCTL_EN | RCTL_BAM | RCTL_SECRC);

        // TX halkası.
        let tx = addr_of_mut!(TX) as *mut TxDesc;
        for i in 0..NTX {
            (*tx.add(i)).addr = 0;
            (*tx.add(i)).status = TX_DD; // boş işaretle
        }
        let tx_phys = addr_of!(TX) as u32;
        reg_w(base, REG_TDBAL, tx_phys);
        reg_w(base, REG_TDBAH, 0);
        reg_w(base, REG_TDLEN, (NTX * 16) as u32);
        reg_w(base, REG_TDH, 0);
        reg_w(base, REG_TDT, 0);
        // CT (çarpışma eşiği)=0x10, COLD (çarpışma penceresi)=0x40, EN|PSP.
        reg_w(base, REG_TCTL, TCTL_EN | TCTL_PSP | (0x10 << 4) | (0x40 << 12));
        reg_w(base, REG_TIPG, 0x0060_200A);

        (*dev).rx_cur = 0;
        (*dev).tx_cur = 0;
        (*dev).ready = true;
    }
    true
}

pub fn ready() -> bool {
    unsafe { (*addr_of!(DEV)).ready }
}

pub fn mac() -> [u8; 6] {
    unsafe { (*addr_of!(DEV)).mac }
}

/// Bir Ethernet çerçevesi gönderir.
pub fn send(frame: &[u8]) {
    let dev = addr_of_mut!(DEV);
    unsafe {
        if !(*dev).ready || frame.len() > BUFSZ {
            return;
        }
        let base = (*dev).base;
        let i = (*dev).tx_cur;

        let buf = addr_of_mut!(TXBUF.0[i]) as *mut u8;
        for (k, &b) in frame.iter().enumerate() {
            write_volatile(buf.add(k), b);
        }

        let tx = addr_of_mut!(TX) as *mut TxDesc;
        (*tx.add(i)).addr = addr_of!(TXBUF.0[i]) as u64;
        (*tx.add(i)).length = frame.len() as u16;
        (*tx.add(i)).cmd = TX_EOP | TX_IFCS | TX_RS;
        (*tx.add(i)).status = 0;

        let next = (i + 1) % NTX;
        (*dev).tx_cur = next;
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        reg_w(base, REG_TDT, next as u32);

        // Tamamlanmayı bekle (DD).
        let mut n = 0u32;
        while read_volatile(addr_of!((*tx.add(i)).status)) & TX_DD == 0 {
            n += 1;
            if n > 5_000_000 {
                break;
            }
            core::hint::spin_loop();
        }
    }
}

/// Bir çerçeve geldiyse `out`a kopyalar ve uzunluğunu döndürür; yoksa `None`.
pub fn recv(out: &mut [u8]) -> Option<usize> {
    let dev = addr_of_mut!(DEV);
    unsafe {
        if !(*dev).ready {
            return None;
        }
        let base = (*dev).base;
        let i = (*dev).rx_cur;
        let rx = addr_of_mut!(RX) as *mut RxDesc;

        let status = read_volatile(addr_of!((*rx.add(i)).status));
        if status & RX_DD == 0 {
            return None;
        }
        let len = read_volatile(addr_of!((*rx.add(i)).length)) as usize;
        let n = core::cmp::min(len, out.len());
        let src = addr_of!(RXBUF.0[i]) as *const u8;
        for (k, b) in out.iter_mut().take(n).enumerate() {
            *b = read_volatile(src.add(k));
        }

        (*rx.add(i)).status = 0;
        reg_w(base, REG_RDT, i as u32);
        (*dev).rx_cur = (i + 1) % NRX;
        Some(n)
    }
}

// --- ARP öz-testi ---

const OUR_IP: [u8; 4] = [10, 0, 2, 15]; // QEMU SLIRP varsayılan istemci
const GW_IP: [u8; 4] = [10, 0, 2, 2]; // QEMU SLIRP ağ geçidi

/// Ağ geçidine bir ARP isteği gönderir ve yanıttaki MAC'i seri porta yazar.
/// Sürücünün hem gönderdiğini hem aldığını kanıtlar.
pub fn selftest_arp() {
    if !ready() {
        return;
    }
    let mac = mac();
    serial::write_str("[e1000] MAC ");
    print_mac(&mac);
    serial::write_str("\n");

    // ARP isteği (42 bayt): Ethernet başlığı + ARP gövdesi.
    let mut f = [0u8; 42];
    f[0..6].copy_from_slice(&[0xFF; 6]); // hedef: broadcast
    f[6..12].copy_from_slice(&mac); // kaynak
    f[12] = 0x08;
    f[13] = 0x06; // EtherType = ARP
    f[14] = 0x00;
    f[15] = 0x01; // HTYPE = Ethernet
    f[16] = 0x08;
    f[17] = 0x00; // PTYPE = IPv4
    f[18] = 6; // HLEN
    f[19] = 4; // PLEN
    f[20] = 0x00;
    f[21] = 0x01; // OPER = istek
    f[22..28].copy_from_slice(&mac); // gönderen MAC
    f[28..32].copy_from_slice(&OUR_IP); // gönderen IP
    // hedef MAC = 0 (bilinmiyor)
    f[38..42].copy_from_slice(&GW_IP); // hedef IP

    serial::write_str("[e1000] ARP istegi gonderiliyor...\n");
    send(&f);

    // Yanıtı bekle.
    let mut buf = [0u8; BUFSZ];
    let mut tries = 0u32;
    loop {
        if let Some(n) = recv(&mut buf) {
            // ARP yanıtı mı? (EtherType 0x0806, OPER=2)
            if n >= 42 && buf[12] == 0x08 && buf[13] == 0x06 && buf[20] == 0x00 && buf[21] == 0x02 {
                serial::write_str("[e1000] ARP yaniti geldi! Gateway MAC ");
                let mut gw = [0u8; 6];
                gw.copy_from_slice(&buf[22..28]);
                print_mac(&gw);
                serial::write_str("\n[e1000] BASARILI: kart paket gonderip aldi.\n");
                return;
            }
        }
        tries += 1;
        if tries > 20_000_000 {
            serial::write_str("[e1000] ARP yaniti gelmedi (zaman asimi).\n");
            return;
        }
        core::hint::spin_loop();
    }
}

fn print_mac(mac: &[u8; 6]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = [0u8; 17];
    let mut p = 0;
    for (i, &b) in mac.iter().enumerate() {
        if i > 0 {
            s[p] = b':';
            p += 1;
        }
        s[p] = HEX[(b >> 4) as usize];
        s[p + 1] = HEX[(b & 0xF) as usize];
        p += 2;
    }
    if let Ok(st) = core::str::from_utf8(&s[..p]) {
        serial::write_str(st);
    }
}
