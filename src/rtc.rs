//! CMOS gerçek zaman saati (RTC) okuyucu.
//!
//! x86'da tarih/saat, CMOS RTC yongasında tutulur ve 0x70 (indeks) / 0x71
//! (veri) portlarından okunur. Değerler genelde BCD biçimindedir; durum
//! yazmacı B'ye (0x0B) bakarak BCD/ikili ve 12/24 saat ayrımını yaparız.

use crate::port::{inb, outb};

const CMOS_ADDR: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub min: u8,
    pub sec: u8,
}

fn read_reg(reg: u8) -> u8 {
    unsafe {
        outb(CMOS_ADDR, reg);
        inb(CMOS_DATA)
    }
}

fn update_in_progress() -> bool {
    read_reg(0x0A) & 0x80 != 0
}

fn read_raw() -> DateTime {
    DateTime {
        sec: read_reg(0x00),
        min: read_reg(0x02),
        hour: read_reg(0x04),
        day: read_reg(0x07),
        month: read_reg(0x08),
        year: read_reg(0x09) as u16,
    }
}

/// Geçerli tarih/saati döndürür. Tutarlılık için güncelleme bitene kadar bekler
/// ve aynı değeri iki kez okuyana dek tekrar eder.
pub fn now() -> DateTime {
    while update_in_progress() {}
    let mut prev = read_raw();
    loop {
        while update_in_progress() {}
        let cur = read_raw();
        if cur == prev {
            break;
        }
        prev = cur;
    }

    let regb = read_reg(0x0B);
    let mut dt = prev;

    // BCD ise ikiliye çevir (durum B bit2 = 0 → BCD).
    if regb & 0x04 == 0 {
        dt.sec = bcd(dt.sec);
        dt.min = bcd(dt.min);
        dt.day = bcd(dt.day);
        dt.month = bcd(dt.month);
        dt.year = bcd(dt.year as u8) as u16;
        // Saatin en üst biti (PM bayrağı) 12 saat kipinde korunur.
        let pm = dt.hour & 0x80 != 0;
        dt.hour = bcd(dt.hour & 0x7F);
        if regb & 0x02 == 0 {
            // 12 saat kipi → 24 saate çevir.
            dt.hour %= 12;
            if pm {
                dt.hour += 12;
            }
        }
    } else if regb & 0x02 == 0 {
        let pm = dt.hour & 0x80 != 0;
        dt.hour &= 0x7F;
        dt.hour %= 12;
        if pm {
            dt.hour += 12;
        }
    }

    dt.year += 2000;
    dt
}

/// Yalnızca saniye alanını ucuz biçimde okur (canlı saat döngüsü için).
pub fn second() -> u8 {
    let s = read_reg(0x00);
    if read_reg(0x0B) & 0x04 == 0 {
        bcd(s)
    } else {
        s
    }
}

#[inline]
fn bcd(v: u8) -> u8 {
    (v & 0x0F) + ((v >> 4) * 10)
}

/// Haftanın gününü tarihten hesaplar (Sakamoto). 0=Pazar .. 6=Cumartesi.
/// CMOS'un gün yazmacı çoğu zaman ayarlanmadığından tarihten türetiriz.
pub fn weekday(dt: &DateTime) -> u8 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = dt.year as i32;
    if dt.month < 3 {
        y -= 1;
    }
    let m = (dt.month as usize).clamp(1, 12) - 1;
    let w = (y + y / 4 - y / 100 + y / 400 + T[m] + dt.day as i32) % 7;
    ((w + 7) % 7) as u8
}
