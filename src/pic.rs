//! Faz 4 — Donanım kesmeleri: 8259A PIC yeniden eşlemesi + PIT zamanlayıcı.
//!
//! IRQ0 (PIT, 100 Hz) uptime ve önleyici zamanlama sağlar. IRQ1 (klavye) tarama
//! kodlarını kesme ile okur. `IrqCtx` yapısı `isr.asm` stub'larıyla uyumludur.

use crate::port::outb;
use crate::serial;
use core::sync::atomic::{AtomicU64, Ordering};

const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;
const EOI: u8 = 0x20;

const PIT_FREQ: u32 = 1_193_182;
/// Saniyedeki zamanlayıcı kesmesi sayısı (10 ms periyot).
pub const HZ: u32 = 100;

static TICKS: AtomicU64 = AtomicU64::new(0);

/// IRQ stub'ının Rust'a ilettiği yığın çerçevesi (pusha + ds/es/fs/gs).
/// Hemen ardından CPU'nun ittiği iret çerçevesi gelir (eip, cs, eflags, …).
#[repr(C)]
pub struct IrqCtx {
    pub gs: u32,
    pub fs: u32,
    pub es: u32,
    pub ds: u32,
    pub edi: u32,
    pub esi: u32,
    pub ebp: u32,
    pub esp: u32,
    pub ebx: u32,
    pub edx: u32,
    pub ecx: u32,
    pub eax: u32,
}

/// Açılıştan bu yana geçen zamanlayıcı kesmesi (tick) sayısı.
pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Zamanlayıcı kesmesi sayacına göre yaklaşık çalışma süresi (ms).
pub fn uptime_ms() -> u64 {
    TICKS.load(Ordering::Relaxed) * (1000 / HZ as u64)
}

/// iret çerçevesindeki alanlara işaretçi (ctx hemen altında).
#[inline]
pub fn iret_frame(ctx: *mut IrqCtx) -> *mut u32 {
    unsafe { (ctx as *mut u8).add(core::mem::size_of::<IrqCtx>()) as *mut u32 }
}

/// Kesme ring 3'ten mi geldi?
#[inline]
pub fn from_user(ctx: *mut IrqCtx) -> bool {
    unsafe { (*iret_frame(ctx).add(1) & 3) == 3 }
}

/// IRQ0 (PIT): tick say, EOI gönder, görev/süreç zamanlayıcılarını çalıştır.
#[no_mangle]
pub extern "C" fn irq0_handler(ctx: *mut IrqCtx) {
    TICKS.fetch_add(1, Ordering::Relaxed);
    unsafe { outb(PIC1_CMD, EOI) };
    crate::user::on_timer(ctx);
    crate::task::on_tick();
}

/// IRQ1 (klavye): şu an maskeli (klavye yoklama ile okunur). Maske açılırsa
/// tarama kodunu okuyup tampona koyar.
#[no_mangle]
pub extern "C" fn irq1_handler() {
    crate::keyboard::irq_feed();
    unsafe { outb(PIC1_CMD, EOI) };
}

/// Beklenmeyen/spurious IRQ: iki PIC'e de EOI gönder ve yok say.
#[no_mangle]
pub extern "C" fn irq_generic_handler() {
    unsafe {
        outb(PIC2_CMD, EOI);
        outb(PIC1_CMD, EOI);
    }
}

#[inline]
fn io_wait() {
    unsafe { outb(0x80, 0) };
}

/// PIC'i yeniden eşler; PIT + klavye (IRQ0/IRQ1) açılır.
pub fn init() {
    unsafe {
        outb(PIC1_CMD, 0x11);
        io_wait();
        outb(PIC2_CMD, 0x11);
        io_wait();
        outb(PIC1_DATA, 0x20);
        io_wait();
        outb(PIC2_DATA, 0x28);
        io_wait();
        outb(PIC1_DATA, 0x04);
        io_wait();
        outb(PIC2_DATA, 0x02);
        io_wait();
        outb(PIC1_DATA, 0x01);
        io_wait();
        outb(PIC2_DATA, 0x01);
        io_wait();

        // Yalnızca IRQ0 (timer) açık. Klavye 8042 tamponunu fare/GUI ile
        // paylaştığından yoklama (polling) ile okunur; IRQ1 + yoklama birlikte
        // aynı porttan okuyup çift girdi yarışına yol açardı.
        outb(PIC1_DATA, 0xFE);
        outb(PIC2_DATA, 0xFF);

        let div = (PIT_FREQ / HZ) as u16;
        outb(0x43, 0x36);
        outb(0x40, (div & 0xFF) as u8);
        outb(0x40, (div >> 8) as u8);
    }
    serial::write_str("[pic] PIC yeniden eslendi; PIT 100 Hz; IRQ0 acik (klavye yoklama)\n");
}

/// Donanım kesmelerini açar (`sti`).
pub fn enable() {
    unsafe { core::arch::asm!("sti", options(nomem, nostack)) };
}
