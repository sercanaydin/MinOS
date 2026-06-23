//! Monotonik milisaniye sayacı.
//!
//! Kesme kullanmadığımız için TSC'yi (rdtsc) PIT kanal 2 ile bir kez kalibre
//! edip, sonrasında geçen süreyi TSC farkından hesaplıyoruz. smoltcp'nin
//! zaman damgaları (zaman aşımları, yeniden gönderim) için yeterli.

use crate::port::{inb, outb};
use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

const PIT_FREQ: u64 = 1_193_182; // Hz

static TSC_PER_MS: AtomicU64 = AtomicU64::new(0);
static TSC_BASE: AtomicU64 = AtomicU64::new(0);

#[inline]
fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack));
    }
    ((hi as u64) << 32) | (lo as u64)
}

/// PIT kanal 2 ile ~50 ms'lik bir aralık ölçerek TSC frekansını kalibre eder.
pub fn init() {
    let count: u16 = (PIT_FREQ * 50 / 1000) as u16; // ~59659 -> 50 ms
    let dt;
    unsafe {
        // Hoparlörü kapat (bit1=0), kapıyı (gate, bit0) aç.
        let p = (inb(0x61) & 0xFD) | 0x01;
        outb(0x61, p);
        // Kanal 2, önce-düşük-sonra-yüksek bayt, mod 0 (terminal count).
        outb(0x43, 0b1011_0000);
        outb(0x42, (count & 0xFF) as u8);
        outb(0x42, (count >> 8) as u8);
        // Kapıyı düşür-kaldır: geri sayımı yeniden başlatır.
        let g = inb(0x61) & 0xFE;
        outb(0x61, g);
        outb(0x61, g | 0x01);

        let t0 = rdtsc();
        // OUT biti (0x20) yükselene kadar bekle (sayım bitti).
        while inb(0x61) & 0x20 == 0 {
            core::hint::spin_loop();
        }
        let t1 = rdtsc();
        dt = t1 - t0;
    }
    let per_ms = (dt / 50).max(1);
    TSC_PER_MS.store(per_ms, Ordering::SeqCst);
    TSC_BASE.store(rdtsc(), Ordering::SeqCst);
}

/// Açılıştan (kalibrasyondan) bu yana geçen milisaniye.
pub fn millis() -> u64 {
    let per = TSC_PER_MS.load(Ordering::SeqCst);
    if per == 0 {
        return 0;
    }
    let base = TSC_BASE.load(Ordering::SeqCst);
    rdtsc().wrapping_sub(base) / per
}
