//! Basit COM1 (16550 UART) seri port sürücüsü.
//!
//! Bu sürücü esas olarak hata ayıklama içindir: QEMU'yu `-serial stdio`
//! ile çalıştırırsak, buraya yazılanlar terminalde görünür. Böylece grafik
//! pencere olmadan da çekirdeğin çalıştığını doğrulayabiliriz.

use crate::port::{inb, outb};

const COM1: u16 = 0x3F8;

pub fn init() {
    unsafe {
        outb(COM1 + 1, 0x00); // kesmeleri kapat
        outb(COM1 + 3, 0x80); // DLAB = 1 (baud bölücüsünü ayarla)
        outb(COM1 + 0, 0x03); // bölücü düşük bayt: 38400 baud
        outb(COM1 + 1, 0x00); // bölücü yüksek bayt
        outb(COM1 + 3, 0x03); // 8 bit, parity yok, 1 stop bit
        outb(COM1 + 2, 0xC7); // FIFO aç, temizle, 14 bayt eşik
        outb(COM1 + 4, 0x0B); // IRQ aç, RTS/DSR ayarla
    }
}

fn is_transmit_empty() -> bool {
    unsafe { inb(COM1 + 5) & 0x20 != 0 }
}

pub fn write_byte(byte: u8) {
    while !is_transmit_empty() {}
    unsafe { outb(COM1, byte) }
}

pub fn write_str(s: &str) {
    for byte in s.bytes() {
        if byte == b'\n' {
            write_byte(b'\r');
        }
        write_byte(byte);
    }
}

/// Bir 32-bit sayıyı `0x` önekiyle, 8 haneli onaltılık olarak yazar.
#[allow(dead_code)]
pub fn write_hex(n: u32) {
    write_str("0x");
    for j in 0..8 {
        let nib = ((n >> ((7 - j) * 4)) & 0xF) as u8;
        write_byte(if nib < 10 {
            b'0' + nib
        } else {
            b'a' + (nib - 10)
        });
    }
}

/// Bir sayıyı ondalık olarak yazar (hata ayıklama için).
#[allow(dead_code)]
pub fn write_dec(mut n: u32) {
    if n == 0 {
        write_byte(b'0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = 0;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        write_byte(buf[i]);
    }
}
