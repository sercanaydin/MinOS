//! Birleşik giriş yoklayıcısı (klavye + fare).
//!
//! Klavye ve fare aynı 8042 çıkış tamponunu paylaşır. Tampondaki sıradaki
//! baytı okumadan bir sonrakine geçemeyiz; bu yüzden tek bir yerden okuyup
//! "aux" (yardımcı) bitine göre fareye veya klavyeye yönlendiririz. Böylece
//! fare baytları klavyeyi (ya da tersi) bozmaz.

use crate::port::inb;
use crate::{keyboard, mouse};

const STATUS: u16 = 0x64;
const DATA: u16 = 0x60;

/// Çıkış tamponundaki tüm bekleyen baytları okuyup yönlendirir.
pub fn poll() {
    loop {
        let status = unsafe { inb(STATUS) };
        if status & 0x01 == 0 {
            break; // okunacak bayt yok
        }
        let data = unsafe { inb(DATA) };
        if status & 0x20 != 0 {
            mouse::feed(data); // aux = fare
        } else {
            keyboard::feed(data); // klavye
        }
    }
}
