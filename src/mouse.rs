//! PS/2 fare sürücüsü (yoklama / polling ile, kesmesiz).
//!
//! Fare, klavye ile aynı 8042 denetleyicisini paylaşır. Veri baytları
//! denetleyicinin çıkış tamponuna "aux" (yardımcı) işaretiyle düşer; bunları
//! `input::poll` ayıklayıp buraya (`feed`) iletir. Üç baytlık paketlerden
//! konum ve düğme durumunu güncelleriz.

use crate::port::{inb, outb};

const DATA: u16 = 0x60;
const STATUS: u16 = 0x64;
const CMD: u16 = 0x64;
const TIMEOUT: u32 = 1_000_000;

static mut X: i32 = 0;
static mut Y: i32 = 0;
static mut MAX_X: i32 = 0;
static mut MAX_Y: i32 = 0;
static mut LEFT: bool = false;
static mut RIGHT: bool = false;

// Paket birleştirme durumu.
static mut PKT: [u8; 3] = [0; 3];
static mut IDX: usize = 0;

fn wait_write() -> bool {
    // Giriş tamponu boşalana kadar bekle (yazmadan önce).
    let mut n = TIMEOUT;
    while n > 0 {
        if unsafe { inb(STATUS) } & 0x02 == 0 {
            return true;
        }
        n -= 1;
    }
    false
}

fn wait_read() -> bool {
    // Çıkış tamponu dolana kadar bekle (okumadan önce).
    let mut n = TIMEOUT;
    while n > 0 {
        if unsafe { inb(STATUS) } & 0x01 != 0 {
            return true;
        }
        n -= 1;
    }
    false
}

unsafe fn write_cmd(byte: u8) {
    wait_write();
    outb(CMD, byte);
}

unsafe fn write_data(byte: u8) {
    wait_write();
    outb(DATA, byte);
}

unsafe fn read_data() -> u8 {
    wait_read();
    inb(DATA)
}

/// Fareye bir komut gönderir ve ACK'i (0xFA) okur.
unsafe fn mouse_cmd(byte: u8) {
    write_cmd(0xD4); // sonraki bayt fareye gitsin
    write_data(byte);
    read_data(); // ACK
}

/// Fareyi başlatır ve veri akışını açar. Ekran sınırlarını da ayarlar.
pub fn init(width: usize, height: usize) {
    unsafe {
        MAX_X = width as i32 - 1;
        MAX_Y = height as i32 - 1;
        X = width as i32 / 2;
        Y = height as i32 / 2;

        write_cmd(0xA8); // yardımcı (fare) aygıtını etkinleştir

        // Denetleyici yapılandırmasını ayarla. Bit'leri AÇIKÇA zorluyoruz; çünkü
        // BIOS'suz (QEMU -kernel) ortamda mevcut değeri okumak güvenilmez ve
        // yanlış değer yazınca klavye bozulur. ÖNEMLİ bit'ler:
        //   bit6 = çeviri (set-2 → set-1) AÇIK olmalı; aksi halde klavye set-2
        //          kodları gönderir ve haritamız (set-1) bozulur.
        //   bit4/bit5 = klavye/fare saati (0 = etkin).
        //   bit0/bit1 = klavye/fare IRQ (yoklasak da zararsız).
        write_cmd(0x20);
        let mut status = read_data();
        status |= 0x47; // bit0+bit1+bit2(sistem)+bit6(çeviri)
        status &= !0x30; // bit4,bit5 temizle (klavye+fare saatini aç)
        write_cmd(0x60);
        write_data(status);

        mouse_cmd(0xF6); // varsayılan ayarlar
        mouse_cmd(0xF4); // veri akışını (streaming) aç

        IDX = 0;
    }
}

/// 8042'den gelen bir fare (aux) baytını işler. `input::poll` çağırır.
pub fn feed(byte: u8) {
    unsafe {
        // İlk baytın 3. biti her zaman 1'dir; senkron için bunu bekleriz.
        if IDX == 0 && byte & 0x08 == 0 {
            return;
        }
        PKT[IDX] = byte;
        IDX += 1;
        if IDX < 3 {
            return;
        }
        IDX = 0;

        let flags = PKT[0];
        // Aşırı taşma varsa paketi at.
        if flags & 0xC0 != 0 {
            return;
        }

        let mut dx = PKT[1] as i32;
        let mut dy = PKT[2] as i32;
        if flags & 0x10 != 0 {
            dx -= 256; // X işaret biti
        }
        if flags & 0x20 != 0 {
            dy -= 256; // Y işaret biti
        }

        X += dx;
        Y -= dy; // fare Y'si yukarı pozitif; ekran Y'si aşağı pozitif
        if X < 0 {
            X = 0;
        }
        if X > MAX_X {
            X = MAX_X;
        }
        if Y < 0 {
            Y = 0;
        }
        if Y > MAX_Y {
            Y = MAX_Y;
        }

        LEFT = flags & 0x01 != 0;
        RIGHT = flags & 0x02 != 0;
    }
}

pub fn x() -> usize {
    unsafe { X as usize }
}

pub fn y() -> usize {
    unsafe { Y as usize }
}

pub fn left() -> bool {
    unsafe { LEFT }
}

#[allow(dead_code)]
pub fn right() -> bool {
    unsafe { RIGHT }
}
