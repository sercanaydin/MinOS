//! Basit ATA/IDE disk sürücüsü (PIO modu, 28-bit LBA).
//!
//! Birincil (primary) ATA veri yolundaki "master" diske 512 baytlık sektörler
//! halinde okuma/yazma yapar. Kesme kullanmadan, durum portunu yoklayarak
//! (polling) çalışır. QEMU'da `-hda disk.img` ile bağlanan disk buraya gelir.

use crate::port::{inb, inw, outb, outw};

const DATA: u16 = 0x1F0; // veri portu (16-bit)
const SECCOUNT: u16 = 0x1F2; // sektör sayısı
const LBA_LO: u16 = 0x1F3;
const LBA_MID: u16 = 0x1F4;
const LBA_HI: u16 = 0x1F5;
const DRIVE: u16 = 0x1F6; // sürücü/kafa seçimi
const STATUS: u16 = 0x1F7; // durum (okuma) / komut (yazma)

const CMD_READ: u8 = 0x20; // READ SECTORS
const CMD_WRITE: u8 = 0x30; // WRITE SECTORS
const CMD_FLUSH: u8 = 0xE7; // FLUSH CACHE

const ST_BSY: u8 = 0x80; // meşgul
const ST_DRQ: u8 = 0x08; // veri transferine hazır
const ST_ERR: u8 = 0x01; // hata

pub const SECTOR_SIZE: usize = 512;

// Disk yanıt vermezse sonsuza dek beklememek için üst sınır.
const TIMEOUT: u32 = 10_000_000;

/// Birincil ATA veri yolunda bir master disk var mı?
/// Disk yoksa durum portu 0xFF döner ("floating bus").
pub fn present() -> bool {
    unsafe {
        outb(DRIVE, 0xE0);
        let s = inb(STATUS);
        s != 0xFF
    }
}

/// Durum portu BSY temizlenip DRQ kuruluncaya kadar bekler.
/// Zaman aşımı ya da hata olursa `false` döner.
fn wait_ready() -> bool {
    unsafe {
        let mut n = 0;
        loop {
            let s = inb(STATUS);
            if s == 0xFF {
                return false; // disk yok
            }
            if s & ST_ERR != 0 {
                return false;
            }
            if s & ST_BSY == 0 && s & ST_DRQ != 0 {
                return true;
            }
            n += 1;
            if n > TIMEOUT {
                return false;
            }
        }
    }
}

fn wait_not_busy() {
    unsafe {
        let mut n = 0;
        while inb(STATUS) & ST_BSY != 0 {
            n += 1;
            if n > TIMEOUT {
                return;
            }
        }
    }
}

/// LBA ve sektör sayısını portlara yazar (28-bit LBA, master sürücü).
unsafe fn setup(lba: u32) {
    wait_not_busy();
    outb(DRIVE, 0xE0 | ((lba >> 24) & 0x0F) as u8); // master + LBA modu
    outb(SECCOUNT, 1);
    outb(LBA_LO, (lba & 0xFF) as u8);
    outb(LBA_MID, ((lba >> 8) & 0xFF) as u8);
    outb(LBA_HI, ((lba >> 16) & 0xFF) as u8);
}

/// Tek bir sektörü `buf` içine okur. Başarılıysa `true`.
pub fn read_sector(lba: u32, buf: &mut [u8; SECTOR_SIZE]) -> bool {
    unsafe {
        setup(lba);
        outb(STATUS, CMD_READ);
        if !wait_ready() {
            return false;
        }
        for i in 0..(SECTOR_SIZE / 2) {
            let word = inw(DATA);
            buf[i * 2] = (word & 0xFF) as u8;
            buf[i * 2 + 1] = (word >> 8) as u8;
        }
        true
    }
}

/// Tek bir sektörü `buf`'tan diske yazar. Başarılıysa `true`.
pub fn write_sector(lba: u32, buf: &[u8; SECTOR_SIZE]) -> bool {
    unsafe {
        setup(lba);
        outb(STATUS, CMD_WRITE);
        if !wait_ready() {
            return false;
        }
        for i in 0..(SECTOR_SIZE / 2) {
            let word = (buf[i * 2] as u16) | ((buf[i * 2 + 1] as u16) << 8);
            outw(DATA, word);
        }
        // Yazma önbelleğini diske boşalt.
        outb(STATUS, CMD_FLUSH);
        wait_not_busy();
        true
    }
}
