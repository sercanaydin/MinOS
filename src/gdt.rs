//! Faz 2 — Global Tanımlayıcı Tablosu (GDT) + Görev Durum Segmenti (TSS).
//!
//! Kullanıcı modu (ring 3) için bize şunlar gerekir:
//!   - Ring 0 (çekirdek) kod/veri segmentleri,
//!   - Ring 3 (kullanıcı) kod/veri segmentleri (DPL=3),
//!   - Bir **TSS**: ring 3 → ring 0 geçişinde (sistem çağrısı / kesme) CPU,
//!     çekirdek yığınını (SS0:ESP0) buradan yükler. TSS olmadan ring 3'ten
//!     kesme alınamaz.
//!
//! Bootloader düz bir GDT bırakmıştı; burada kendi GDT'mizi kurup yüklüyoruz.
//! Segment seçicileri: kod 0x08, veri 0x10, kullanıcı kod 0x1B (=0x18|3),
//! kullanıcı veri 0x23 (=0x20|3), TSS 0x28.

use crate::serial;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct GdtEntry {
    limit_lo: u16,
    base_lo: u16,
    base_mid: u8,
    access: u8,
    gran: u8,
    base_hi: u8,
}

const fn entry(base: u32, limit: u32, access: u8, flags: u8) -> GdtEntry {
    GdtEntry {
        limit_lo: (limit & 0xFFFF) as u16,
        base_lo: (base & 0xFFFF) as u16,
        base_mid: ((base >> 16) & 0xFF) as u8,
        access,
        gran: (((flags & 0x0F) << 4) | ((limit >> 16) & 0x0F) as u8),
        base_hi: ((base >> 24) & 0xFF) as u8,
    }
}

#[repr(C, packed)]
struct Gdtr {
    limit: u16,
    base: u32,
}

/// 32-bit Görev Durum Segmenti. Yalnızca SS0/ESP0 alanlarını kullanıyoruz.
#[repr(C, packed)]
struct Tss {
    link: u32,
    esp0: u32,
    ss0: u32,
    esp1: u32,
    ss1: u32,
    esp2: u32,
    ss2: u32,
    cr3: u32,
    eip: u32,
    eflags: u32,
    eax: u32,
    ecx: u32,
    edx: u32,
    ebx: u32,
    esp: u32,
    ebp: u32,
    esi: u32,
    edi: u32,
    es: u32,
    cs: u32,
    ss: u32,
    ds: u32,
    fs: u32,
    gs: u32,
    ldt: u32,
    trap: u16,
    iomap: u16,
}

static mut GDT: [GdtEntry; 6] = [entry(0, 0, 0, 0); 6];

static mut TSS: Tss = Tss {
    link: 0,
    esp0: 0,
    ss0: 0,
    esp1: 0,
    ss1: 0,
    esp2: 0,
    ss2: 0,
    cr3: 0,
    eip: 0,
    eflags: 0,
    eax: 0,
    ecx: 0,
    edx: 0,
    ebx: 0,
    esp: 0,
    ebp: 0,
    esi: 0,
    edi: 0,
    es: 0,
    cs: 0,
    ss: 0,
    ds: 0,
    fs: 0,
    gs: 0,
    ldt: 0,
    trap: 0,
    iomap: 0,
}; // iomap init() içinde ayarlanır

#[repr(align(16))]
#[allow(dead_code)]
struct KStack([u8; 16384]);

/// Ring 3 → ring 0 geçişlerinde kullanılan çekirdek yığını (TSS.ESP0).
static mut KSTACK: KStack = KStack([0; 16384]);

/// GDT + TSS'i kurar ve yükler. `idt::init`'ten ÖNCE çağrılmalı (IDT, geçerli
/// CS seçicisini kullanır).
pub fn init() {
    unsafe {
        let g = &mut *core::ptr::addr_of_mut!(GDT);
        g[0] = entry(0, 0, 0, 0); // null
        g[1] = entry(0, 0xFFFFF, 0x9A, 0xC); // çekirdek kod (ring0)
        g[2] = entry(0, 0xFFFFF, 0x92, 0xC); // çekirdek veri (ring0)
        g[3] = entry(0, 0xFFFFF, 0xFA, 0xC); // kullanıcı kod (ring3)
        g[4] = entry(0, 0xFFFFF, 0xF2, 0xC); // kullanıcı veri (ring3)

        let tss = &mut *core::ptr::addr_of_mut!(TSS);
        tss.ss0 = 0x10;
        tss.esp0 = core::ptr::addr_of!(KSTACK) as u32 + 16384;
        tss.iomap = core::mem::size_of::<Tss>() as u16; // G/Ç izin haritası yok

        let tss_base = core::ptr::addr_of!(TSS) as u32;
        let tss_limit = core::mem::size_of::<Tss>() as u32 - 1;
        // access 0x89 = present, type=9 (32-bit kullanılabilir TSS), DPL0
        g[5] = entry(tss_base, tss_limit, 0x89, 0x0);

        let gdtr = Gdtr {
            limit: (core::mem::size_of::<[GdtEntry; 6]>() - 1) as u16,
            base: core::ptr::addr_of!(GDT) as u32,
        };

        core::arch::asm!(
            "lgdt [{gdtr}]",
            "mov ax, 0x10",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            "mov ss, ax",
            // CS'yi uzak dönüşle (retf) yeniden yükle.
            "push 0x08",
            "lea eax, [2f]",
            "push eax",
            "retf",
            "2:",
            "mov ax, 0x28",
            "ltr ax",
            gdtr = in(reg) &gdtr,
            out("eax") _,
        );
    }
    serial::write_str("[gdt] GDT+TSS yuklendi (ring0/ring3 segmentleri)\n");
}
