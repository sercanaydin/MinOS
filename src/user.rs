//! Faz 3 — Kullanıcı modu (ring 3) + ELF yükleme demoları.
//!
//! `run_hello`: derleme anında üretilen ayrı bir ELF programını
//! (`userprog.elf`, bkz. `src/userprog.asm`) ELF yükleyiciyle kullanıcı adres
//! alanına yükler, ring 3'te çalıştırır; program `int 0x80` ile kendi
//! belleğindeki metni yazdırır ve `exit(7)` ile çekirdeğe döner.
//!
//! `run_fault`: kullanıcı modundan bir ÇEKİRDEK sayfasına yazmayı dener →
//! sayfa hatası (#PF) → panik. Bellek korumasının kanıtı.

use crate::vga::{self, Color};
use crate::{elf, mem};

extern "C" {
    fn enter_user_mode(entry: u32, user_esp: u32);
    static kernel_resume_esp: u32;
}
static USERPROG_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/userprog.elf"));
/// clang ile derlenen C uygulamaları (boş ise clang yoktu).
static HELLO_C_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/hello_c.elf"));
static CALC_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/calc.elf"));
static PAINT_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/paint.elf"));
/// c4: OS içinde çalışan küçük C derleyici+VM (Robert Swierczek, MIT).
static C4_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/c4.elf"));

/// Diske kurulabilen gömülü C uygulamaları: (disk adı, ELF).
pub fn c_apps() -> [(&'static str, &'static [u8]); 4] {
    // Not: disk adları, MinOS'un Türkçe-Q klavyesinde kolay yazılabilsin diye
    // 'i' harfi içermez ('i' tuşu noktasız 'ı' üretir). "boya" = paint.
    [
        ("web", HELLO_C_ELF),
        ("calc", CALC_ELF),
        ("boya", PAINT_ELF),
        ("c4", C4_ELF),
    ]
}

/// `cprog` komutu: gömülü örnek C programını (web tarayıcı) ring 3'te çalıştırır.
pub fn run_c_demo() {
    if HELLO_C_ELF.len() < 52 {
        vga::set_color(Color::LightRed, Color::Black);
        crate::println!("C programi gomulu degil (clang ile yeniden derleyin: make build).");
        vga::set_color(Color::LightGray, Color::Black);
        return;
    }
    run_elf(HELLO_C_ELF);
}

// Çekirdek sayfasına (0x0010_0000) yazmayı deneyen ham program (koruma sınaması):
//   mov dword [0x00100000], 0x0000DEAD ; jmp $
#[rustfmt::skip]
const PROG_FAULT: &[u8] = &[
    0xC7, 0x05, 0x00, 0x00, 0x10, 0x00, 0xAD, 0xDE, 0x00, 0x00,
    0xEB, 0xFE,
];

/// Çekirdeğe gömülü örnek ELF programı (diske yazıp `run` ile çalıştırmak için).
pub fn embedded_elf() -> &'static [u8] {
    USERPROG_ELF
}

/// `ring3` komutu: gömülü ELF programını yükler ve ring 3'te çalıştırır.
pub fn run_hello() {
    run_elf(USERPROG_ELF);
}

/// Bir ELF imajını (gömülü ya da diskten okunmuş) ring 3'te çalıştırır.
///
/// Not: `elf::load` hata durumunda kendi temizliğini (user_end) yapar; bu yüzden
/// hata dalında tekrar `user_end` ÇAĞIRMAYIZ. Başarıda temizlik bizim görevimiz.
pub fn run_elf(image: &[u8]) {
    vga::set_color(Color::LightCyan, Color::Black);
    crate::println!("ELF çalıştırılıyor ({} bayt):", image.len());
    vga::set_color(Color::LightGray, Color::Black);

    match elf::load(image) {
        Ok((entry, esp)) => {
            let pid = crate::idt::begin_process();
            crate::println!("  PID={pid}, giriş: {entry:#010x}, yığın: {esp:#010x} — CPL=3'e geçiliyor:");
            crate::println!("  ----------------------------------------");
            unsafe { enter_user_mode(entry, esp) };
            crate::println!("  ----------------------------------------");
            let code = crate::idt::user_exit_code();
            mem::user_end();
            vga::set_color(Color::LightGreen, Color::Black);
            crate::println!("  -> exit({code}); çekirdeğe güvenle dönüldü.");
            vga::set_color(Color::LightGray, Color::Black);
        }
        Err(e) => {
            vga::set_color(Color::LightRed, Color::Black);
            crate::println!("  ELF yüklenemedi: {e}");
            vga::set_color(Color::LightGray, Color::Black);
        }
    }
}

/// Ring 3'ten bir çekirdek sayfasına yazmayı dener → koruma hatası (panik).
pub fn run_fault() {
    vga::set_color(Color::Yellow, Color::Black);
    crate::println!("Bellek koruması sınaması:");
    vga::set_color(Color::LightGray, Color::Black);
    crate::println!("  Ring 3'ten çekirdek belleğine (0x00100000) yazılmaya çalışılacak.");
    crate::println!("  Beklenen: sayfa hatası (#PF) -> panik ekranı (sistem korunur).");

    mem::user_begin();
    let frame = mem::user_map_page(mem::USER_BASE, false);
    unsafe {
        core::ptr::copy_nonoverlapping(PROG_FAULT.as_ptr(), frame as *mut u8, PROG_FAULT.len());
    }
    let esp = mem::user_setup_stack();
    mem::user_flush();

    unsafe { enter_user_mode(mem::USER_BASE, esp) };

    // Buraya normalde ulaşılmaz (panik ekranında durulur).
    mem::user_end();
}

// --- Ring 3 önleyici çok süreç (ayrı sayfa tabloları) ---

const COUNTER_OFF: u32 = 8;
/// Sonsuz döngüde sayaç artıran mini ring3 program (her süreçte ayrı fiziksel sayfa).
#[rustfmt::skip]
const PROG_COUNTER: &[u8] = &[
    0xFF, 0x05, 0x08, 0x00, 0x00, 0x40, // inc dword [0x40000008]
    0xEB, 0xF8,                         // jmp short -8
];

const MAX_UPROCS: usize = 2;

struct UserProc {
    space: mem::UserSpace,
    saved_eip: u32,
    saved_esp: u32,
    saved_eflags: u32,
    alive: bool,
}

impl UserProc {
    const EMPTY: Self = Self {
        space: mem::UserSpace::EMPTY,
        saved_eip: 0,
        saved_esp: 0,
        saved_eflags: 0,
        alive: false,
    };
}

static mut UPROCS: [UserProc; MAX_UPROCS] = [UserProc::EMPTY; MAX_UPROCS];
static mut NUPROCS: usize = 0;
static mut UCUR: usize = 0;
static mut UPREEMPT: bool = false;
static mut UEND_TICK: u64 = 0;
static mut URESULTS: [u32; MAX_UPROCS] = [0; MAX_UPROCS];

fn kernel_cs() -> u32 {
    let cs: u16;
    unsafe {
        core::arch::asm!("mov {0:x}, cs", out(reg) cs, options(nomem, nostack, preserves_flags));
    }
    cs as u32
}

fn pick_uproc(cur: usize) -> usize {
    unsafe {
        for off in 1..=NUPROCS {
            let c = (cur + off) % NUPROCS;
            if UPROCS[c].alive {
                return c;
            }
        }
        cur
    }
}

fn setup_counter_proc(space: &mut mem::UserSpace) -> u32 {
    let frame = space.map_page(mem::USER_BASE, true);
    unsafe {
        core::ptr::copy_nonoverlapping(PROG_COUNTER.as_ptr(), frame as *mut u8, PROG_COUNTER.len());
    }
    space.setup_stack()
}

/// Zamanlayıcı kesmesi: ring 3'teyken süreçler arası zorla geçiş.
pub fn on_timer(ctx: *mut crate::pic::IrqCtx) {
    if !unsafe { UPREEMPT } || !crate::pic::from_user(ctx) {
        return;
    }
    let now = crate::pic::ticks();
    unsafe {
        let f = crate::pic::iret_frame(ctx);
        let cur = UCUR;
        UPROCS[cur].saved_eip = *f.add(0);
        UPROCS[cur].saved_esp = *f.add(3);
        UPROCS[cur].saved_eflags = *f.add(2);

        if now >= UEND_TICK {
            UPREEMPT = false;
            for i in 0..NUPROCS {
                if UPROCS[i].alive {
                    UPROCS[i].space.activate();
                    URESULTS[i] = UPROCS[i].space.read_u32(mem::USER_BASE + COUNTER_OFF);
                }
            }
            mem::user_restore_identity();
            *f.add(0) = user_demo_done as *const () as u32;
            *f.add(1) = kernel_cs();
            *f.add(2) = 0x202;
            return;
        }

        let next = pick_uproc(cur);
        if next == cur {
            return;
        }
        UCUR = next;
        UPROCS[next].space.activate();
        *f.add(0) = UPROCS[next].saved_eip;
        *f.add(1) = 0x1B;
        *f.add(2) = UPROCS[next].saved_eflags | 0x200;
        *f.add(3) = UPROCS[next].saved_esp;
        *f.add(4) = 0x23;
    }
}

fn print_preempt_results() {
    unsafe {
        for i in 0..NUPROCS {
            crate::println!("  [surec {}] sayac = {}", i + 1, URESULTS[i]);
            UPROCS[i].space.destroy();
            UPROCS[i].alive = false;
        }
        NUPROCS = 0;
    }
}

/// Demo bittiğinde timer tarafından ring 0'a atlanır; `enter_user_mode` kaydına döner.
#[no_mangle]
pub extern "C" fn user_demo_done() {
    print_preempt_results();
    mem::user_restore_identity();
    unsafe {
        core::arch::asm!(
            "mov esp, dword ptr [{esp_slot}]",
            "popf",
            "popa",
            "ret",
            esp_slot = sym kernel_resume_esp,
            options(nomem, nostack)
        );
    }
}

/// İki ring3 süreci ayrı sayfa tablolarında çalışır; timer (~1.5 sn) zorla böler.
pub fn run_preempt_demo() {
    vga::set_color(Color::LightCyan, Color::Black);
    crate::println!("Ring 3 onleyici cok surec (ayri sayfa tablolari):");
    vga::set_color(Color::LightGray, Color::Black);
    crate::println!("  2 surec ayni sanal adreste sayac artiriyor; timer zorla degistiriyor.");
    crate::println!("  ----------------------------------------");

    unsafe {
        NUPROCS = 2;
        for i in 0..NUPROCS {
            let mut space = mem::UserSpace::new();
            let esp = setup_counter_proc(&mut space);
            UPROCS[i] = UserProc {
                space,
                saved_eip: mem::USER_BASE,
                saved_esp: esp,
                saved_eflags: 0x202,
                alive: true,
            };
        }
        mem::save_user_pde_slot();
        UCUR = 0;
        UPREEMPT = true;
        UEND_TICK = crate::pic::ticks() + 150;
        UPROCS[0].space.activate();
        enter_user_mode(mem::USER_BASE, UPROCS[0].saved_esp);
        UPREEMPT = false;
    }

    vga::set_color(Color::LightGreen, Color::Black);
    crate::println!("  ----------------------------------------");
    crate::println!("  -> timer ring3 surecleri kesti; sayaclar yukarida (ayri bellek).");
    vga::set_color(Color::LightGray, Color::Black);
}
