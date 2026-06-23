//! Minimal NVMe (PCIe SSD) sürücüsü — yoklama (polling) tabanlı.
//!
//! Tek bir admin ve tek bir G/Ç kuyruğu kurar, 1. ad alanını (namespace)
//! tanır ve 512 baytlık mantıksal bloklar halinde okuma/yazma yapar. Kesme
//! kullanmaz; tamamlanmayı faz bitini yoklayarak bekler.
//!
//! Bellek modeli: çekirdek 32-bit korumalı modda, sayfalama (paging) KAPALI
//! çalışır; bu yüzden bir static'in adresi doğrudan fiziksel adrestir ve DMA
//! tamponları/PRP'ler için kullanılabilir. x86'da DMA önbellek-tutarlıdır.

#![allow(dead_code)]

use crate::ata::SECTOR_SIZE;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

const QDEPTH: usize = 8; // kuyruk derinliği (giriş sayısı)
const TIMEOUT: u32 = 50_000_000;

#[repr(C, align(4096))]
struct Page([u8; 4096]);

static mut ASQ: Page = Page([0; 4096]); // admin gönderim kuyruğu
static mut ACQ: Page = Page([0; 4096]); // admin tamamlanma kuyruğu
static mut IOSQ: Page = Page([0; 4096]); // G/Ç gönderim kuyruğu
static mut IOCQ: Page = Page([0; 4096]); // G/Ç tamamlanma kuyruğu
static mut DATA: Page = Page([0; 4096]); // tek blok DMA tamponu
static mut IDENT: Page = Page([0; 4096]); // identify çıktısı

struct Nvme {
    base: usize,
    dstrd: u32,
    cid: u16,
    asq_tail: u16,
    acq_head: u16,
    acq_phase: u32,
    iosq_tail: u16,
    iocq_head: u16,
    iocq_phase: u32,
    nlbas: u64,
    lba_bytes: u32,
    detected: bool,
    ready: bool,
}

static mut NV: Nvme = Nvme {
    base: 0,
    dstrd: 0,
    cid: 0,
    asq_tail: 0,
    acq_head: 0,
    acq_phase: 1,
    iosq_tail: 0,
    iocq_head: 0,
    iocq_phase: 1,
    nlbas: 0,
    lba_bytes: 0,
    detected: false,
    ready: false,
};

#[inline]
unsafe fn mr32(base: usize, off: usize) -> u32 {
    read_volatile((base + off) as *const u32)
}

#[inline]
unsafe fn mw32(base: usize, off: usize, v: u32) {
    write_volatile((base + off) as *mut u32, v);
}

#[inline]
fn db_off(dstrd: u32, qid: usize, is_cq: bool) -> usize {
    let stride = 4usize << dstrd;
    0x1000 + (2 * qid + if is_cq { 1 } else { 0 }) * stride
}

/// 64 baytlık komutu kuyruk sayfasındaki `slot`a yazar.
unsafe fn write_cmd(sq: *mut u8, slot: usize, cmd: &[u32; 16]) {
    let p = sq.add(slot * 64) as *mut u32;
    for (i, &w) in cmd.iter().enumerate() {
        write_volatile(p.add(i), w);
    }
}

/// Bir tamamlanma kuyruğunda sıradaki girişi bekler, durum alanını döndürür.
/// 0 = başarı. Zaman aşımında 0xFFFF.
unsafe fn wait_cq(base: usize, cq: *const u8, head: &mut u16, phase: &mut u32, db: usize) -> u16 {
    let mut n = 0u32;
    loop {
        let entry = cq.add(*head as usize * 16) as *const u32;
        let dw3 = read_volatile(entry.add(3));
        if (dw3 >> 16) & 1 == *phase {
            let status = ((dw3 >> 17) & 0x7FFF) as u16;
            *head = (*head + 1) % QDEPTH as u16;
            if *head == 0 {
                *phase ^= 1;
            }
            mw32(base, db, *head as u32);
            return status;
        }
        n += 1;
        if n > TIMEOUT {
            return 0xFFFF;
        }
    }
}

unsafe fn admin_cmd(nv: *mut Nvme, mut cmd: [u32; 16]) -> u16 {
    let base = (*nv).base;
    (*nv).cid = (*nv).cid.wrapping_add(1);
    cmd[0] = (cmd[0] & 0x0000_FFFF) | ((((*nv).cid) as u32) << 16);

    let slot = (*nv).asq_tail as usize;
    write_cmd(addr_of_mut!(ASQ) as *mut u8, slot, &cmd);
    (*nv).asq_tail = ((*nv).asq_tail + 1) % QDEPTH as u16;

    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    mw32(base, db_off((*nv).dstrd, 0, false), (*nv).asq_tail as u32);

    let mut head = (*nv).acq_head;
    let mut phase = (*nv).acq_phase;
    let st = wait_cq(
        base,
        addr_of!(ACQ) as *const u8,
        &mut head,
        &mut phase,
        db_off((*nv).dstrd, 0, true),
    );
    (*nv).acq_head = head;
    (*nv).acq_phase = phase;
    st
}

unsafe fn io_cmd(nv: *mut Nvme, mut cmd: [u32; 16]) -> u16 {
    let base = (*nv).base;
    (*nv).cid = (*nv).cid.wrapping_add(1);
    cmd[0] = (cmd[0] & 0x0000_FFFF) | ((((*nv).cid) as u32) << 16);

    let slot = (*nv).iosq_tail as usize;
    write_cmd(addr_of_mut!(IOSQ) as *mut u8, slot, &cmd);
    (*nv).iosq_tail = ((*nv).iosq_tail + 1) % QDEPTH as u16;

    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    mw32(base, db_off((*nv).dstrd, 1, false), (*nv).iosq_tail as u32);

    let mut head = (*nv).iocq_head;
    let mut phase = (*nv).iocq_phase;
    let st = wait_cq(
        base,
        addr_of!(IOCQ) as *const u8,
        &mut head,
        &mut phase,
        db_off((*nv).dstrd, 1, true),
    );
    (*nv).iocq_head = head;
    (*nv).iocq_phase = phase;
    st
}

/// NVMe denetleyicisini bulur ve başlatır. 512 baytlık mantıksal blok kullanan
/// bir ad alanı bulursa `true` döner.
pub fn init() -> bool {
    let base = match crate::pci::find_nvme() {
        Some(b) => b as usize,
        None => return false,
    };

    let nv = addr_of_mut!(NV);
    unsafe {
        (*nv).base = base;

        // CAP (64-bit): DSTRD = bit 32..35 → cap_hi'nin 0..3 bitleri.
        let cap_hi = mr32(base, 0x04);
        (*nv).dstrd = cap_hi & 0xF;

        // Denetleyiciyi kapat ve hazır-değil olana dek bekle.
        mw32(base, 0x14, 0);
        let mut n = 0u32;
        while mr32(base, 0x1C) & 1 != 0 {
            n += 1;
            if n > TIMEOUT {
                return false;
            }
        }

        // Admin kuyruk boyutları (AQA) ve adresleri (ASQ/ACQ).
        mw32(base, 0x24, (((QDEPTH - 1) as u32) << 16) | ((QDEPTH - 1) as u32));
        let asq = addr_of!(ASQ) as u64;
        let acq = addr_of!(ACQ) as u64;
        mw32(base, 0x28, asq as u32);
        mw32(base, 0x2C, (asq >> 32) as u32);
        mw32(base, 0x30, acq as u32);
        mw32(base, 0x34, (acq >> 32) as u32);

        // CC: IOCQES=4 (16B), IOSQES=6 (64B), MPS=0 (4KB), CSS=0 (NVM), EN=1.
        mw32(base, 0x14, (4 << 20) | (6 << 16) | 1);

        // Hazır olana dek bekle (CSTS.RDY=1); ölümcül hata (CFS) olursa çık.
        n = 0;
        loop {
            let csts = mr32(base, 0x1C);
            if csts & 0x2 != 0 {
                return false; // CFS: denetleyici ölümcül hata
            }
            if csts & 0x1 != 0 {
                break;
            }
            n += 1;
            if n > TIMEOUT {
                return false;
            }
        }

        // G/Ç tamamlanma kuyruğu oluştur (admin opcode 0x05).
        let iocq = addr_of!(IOCQ) as u64;
        let mut c = [0u32; 16];
        c[0] = 0x05;
        c[6] = iocq as u32;
        c[7] = (iocq >> 32) as u32;
        c[10] = (((QDEPTH - 1) as u32) << 16) | 1; // CQ boyutu | qid=1
        c[11] = 0x0001; // PC=1, IEN=0 (yoklama)
        if admin_cmd(nv, c) != 0 {
            return false;
        }

        // G/Ç gönderim kuyruğu oluştur (admin opcode 0x01).
        let iosq = addr_of!(IOSQ) as u64;
        c = [0u32; 16];
        c[0] = 0x01;
        c[6] = iosq as u32;
        c[7] = (iosq >> 32) as u32;
        c[10] = (((QDEPTH - 1) as u32) << 16) | 1; // SQ boyutu | qid=1
        c[11] = (1 << 16) | 1; // CQID=1 | PC=1
        if admin_cmd(nv, c) != 0 {
            return false;
        }

        // 1. ad alanını tanı (admin opcode 0x06, CNS=0).
        let ident = addr_of!(IDENT) as u64;
        c = [0u32; 16];
        c[0] = 0x06;
        c[1] = 1; // NSID=1
        c[6] = ident as u32;
        c[7] = (ident >> 32) as u32;
        c[10] = 0; // CNS=0: belirtilen ad alanı
        if admin_cmd(nv, c) != 0 {
            return false;
        }

        let id = addr_of!(IDENT) as *const u8;
        let mut nsze: u64 = 0;
        for i in 0..8 {
            nsze |= (read_volatile(id.add(i)) as u64) << (8 * i);
        }
        let flbas = read_volatile(id.add(26));
        // LBA format girişi (32-bit) 128 + (flbas&0xF)*4 ofsetinde; LBADS = bit16..23.
        let lbaf_off = 128 + (flbas as usize & 0xF) * 4;
        let mut lbaf: u32 = 0;
        for i in 0..4 {
            lbaf |= (read_volatile(id.add(lbaf_off + i)) as u32) << (8 * i);
        }
        let lbads = (lbaf >> 16) & 0xFF;
        (*nv).lba_bytes = 1u32 << lbads;
        (*nv).nlbas = nsze;
        (*nv).detected = true;

        if (*nv).lba_bytes != SECTOR_SIZE as u32 {
            // Dosya sistemimiz 512 baytlık sektör bekliyor.
            return false;
        }

        (*nv).ready = true;
        true
    }
}

/// NVMe hazır mı (bulundu ve 512B blok)?
pub fn ready() -> bool {
    unsafe { (*addr_of!(NV)).ready }
}

/// NVMe denetleyicisi bulunup tanındı mı (blok boyutundan bağımsız)?
pub fn detected() -> bool {
    unsafe { (*addr_of!(NV)).detected }
}

/// Ad alanının mantıksal blok boyutu (bayt). Tanınmadıysa 0.
pub fn lba_bytes() -> u32 {
    unsafe { (*addr_of!(NV)).lba_bytes }
}

/// Ad alanının 512 baytlık sektör cinsinden boyutu (u32'ye sığacak şekilde).
pub fn sectors() -> u32 {
    let n = unsafe { (*addr_of!(NV)).nlbas };
    if n > u32::MAX as u64 {
        u32::MAX
    } else {
        n as u32
    }
}

/// Tek bir 512B sektörü okur.
pub fn read_sector(lba: u32, buf: &mut [u8; SECTOR_SIZE]) -> bool {
    let nv = addr_of_mut!(NV);
    unsafe {
        if !(*nv).ready {
            return false;
        }
        let datap = addr_of!(DATA) as u64;
        let mut c = [0u32; 16];
        c[0] = 0x02; // READ
        c[1] = 1; // NSID
        c[6] = datap as u32;
        c[7] = (datap >> 32) as u32;
        c[10] = lba; // SLBA düşük
        c[11] = 0; // SLBA yüksek
        c[12] = 0; // NLB=0 → 1 blok
        if io_cmd(nv, c) != 0 {
            return false;
        }
        let src = addr_of!(DATA) as *const u8;
        for (i, b) in buf.iter_mut().enumerate() {
            *b = read_volatile(src.add(i));
        }
        true
    }
}

/// Tek bir 512B sektörü yazar.
pub fn write_sector(lba: u32, buf: &[u8; SECTOR_SIZE]) -> bool {
    let nv = addr_of_mut!(NV);
    unsafe {
        if !(*nv).ready {
            return false;
        }
        let dst = addr_of_mut!(DATA) as *mut u8;
        for (i, &b) in buf.iter().enumerate() {
            write_volatile(dst.add(i), b);
        }
        let datap = addr_of!(DATA) as u64;
        let mut c = [0u32; 16];
        c[0] = 0x01; // WRITE
        c[1] = 1; // NSID
        c[6] = datap as u32;
        c[7] = (datap >> 32) as u32;
        c[10] = lba;
        c[11] = 0;
        c[12] = 0; // NLB=0 → 1 blok
        io_cmd(nv, c) == 0
    }
}
