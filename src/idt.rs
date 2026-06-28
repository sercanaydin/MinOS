//! Faz 2 — Kesme Tanımlayıcı Tablosu (IDT) ve CPU istisna işleme.
//!
//! Şimdiye kadar çekirdeğin hiç kesme altyapısı yoktu: bir CPU istisnası (örn.
//! sayfa hatası) oluşursa işlemci üçlü hata (triple fault) verip makineyi yeniden
//! başlatıyordu. Kullanıcı alanı (Faz 2 hedefi) için ön koşul, istisnaları
//! yakalayabilmektir — özellikle **sayfa hatası (#PF)**, çünkü bellek korumasının
//! ve hatalı erişimlerin (segfault) temeli budur.
//!
//! Burada 32 CPU istisnası için bir IDT kurar, `lidt` ile yükler ve `isr.asm`'deki
//! stub'lar üzerinden hepsini tek bir Rust işleyicisine yönlendiririz. Donanım
//! IRQ'larını (PIC) **etkinleştirmiyoruz** — klavye hâlâ yoklama (polling) ile
//! okunuyor; yalnızca CPU istisnalarını ele alıyoruz.

use crate::mem;
use crate::serial;
use crate::vga::{self, Color};

extern "C" {
    /// isr.asm: 32 stub adresinden oluşan tablo.
    static isr_stub_table: [u32; 32];
    /// isr.asm: sistem çağrısı (int 0x80) stub'ı.
    fn isr_syscall();
    /// isr.asm: IRQ0 (PIT zamanlayıcı) stub'ı.
    fn irq0_stub();
    /// isr.asm: IRQ1 (klavye) stub'ı.
    fn irq1_stub();
    /// isr.asm: diğer IRQ'lar için ortak (yok say + EOI) stub.
    fn irq_generic_stub();
}

// --- IDT yapıları ---

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    off_lo: u16,
    sel: u16,
    zero: u8,
    attr: u8,
    off_hi: u16,
}

impl IdtEntry {
    const fn missing() -> Self {
        Self {
            off_lo: 0,
            sel: 0,
            zero: 0,
            attr: 0,
            off_hi: 0,
        }
    }
}

#[repr(C, packed)]
struct Idtr {
    limit: u16,
    base: u32,
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::missing(); 256];

/// `isr_common_handler`'ın aldığı yığın çerçevesi. Düzen `isr.asm` ile birebir
/// uyumlu olmalı (önce gs/fs/es/ds, sonra pusha, sonra numara/hata kodu, sonra
/// CPU'nun ittiği eip/cs/eflags).
#[repr(C)]
pub struct IsrFrame {
    gs: u32,
    fs: u32,
    es: u32,
    ds: u32,
    edi: u32,
    esi: u32,
    ebp: u32,
    esp_dummy: u32,
    ebx: u32,
    edx: u32,
    ecx: u32,
    eax: u32,
    int_num: u32,
    err_code: u32,
    eip: u32,
    cs: u32,
    eflags: u32,
}

fn set_gate(i: usize, handler: u32, sel: u16, attr: u8) {
    unsafe {
        let idt = &mut *core::ptr::addr_of_mut!(IDT);
        idt[i] = IdtEntry {
            off_lo: (handler & 0xFFFF) as u16,
            sel,
            zero: 0,
            attr,
            off_hi: (handler >> 16) as u16,
        };
    }
}

/// IDT'yi kurup yükler. `mem`/`allocator`'dan sonra, shell'den önce çağrılmalı.
pub fn init() {
    // Bootloader'ın kurduğu çekirdek kod segman seçicisini (CS) kullan.
    let cs: u16;
    unsafe {
        core::arch::asm!("mov {0:x}, cs", out(reg) cs, options(nomem, nostack, preserves_flags));
    }

    unsafe {
        let table = &*core::ptr::addr_of!(isr_stub_table);
        for i in 0..32 {
            // 0x8E = present, DPL=0, 32-bit kesme kapısı (interrupt gate).
            set_gate(i, table[i], cs, 0x8E);
        }

        // Donanım IRQ kapıları (PIC yeniden eşlemesi sonrası 0x20..0x2F).
        // 0x20 = IRQ0 (PIT timer); gerisi beklenmeyen IRQ'lar için ortak stub.
        set_gate(0x20, irq0_stub as *const () as u32, cs, 0x8E);
        set_gate(0x21, irq1_stub as *const () as u32, cs, 0x8E);
        for v in 0x22..=0x2F {
            set_gate(v, irq_generic_stub as *const () as u32, cs, 0x8E);
        }

        // Sistem çağrısı kapısı: int 0x80, DPL=3 (ring 3'ten çağrılabilsin).
        // 0xEE = present, DPL=3, 32-bit kesme kapısı.
        set_gate(0x80, isr_syscall as *const () as u32, cs, 0xEE);

        let idtr = Idtr {
            limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
            base: core::ptr::addr_of!(IDT) as u32,
        };
        core::arch::asm!("lidt [{}]", in(reg) &idtr, options(readonly, nostack, preserves_flags));
    }

    serial::write_str("[idt] kesme tablosu yuklendi (32 CPU istisnasi)\n");
}

const NAMES: [&str; 32] = [
    "Bolme hatasi (#DE)",
    "Debug (#DB)",
    "NMI",
    "Breakpoint (#BP)",
    "Overflow (#OF)",
    "BOUND asimi (#BR)",
    "Gecersiz opcode (#UD)",
    "Aygit yok (#NM)",
    "Cift hata (#DF)",
    "Yardimci islemci asimi",
    "Gecersiz TSS (#TS)",
    "Segman yok (#NP)",
    "Yigin segman hatasi (#SS)",
    "Genel koruma hatasi (#GP)",
    "Sayfa hatasi (#PF)",
    "Ayrilmis",
    "x87 FPU hatasi (#MF)",
    "Hizalama kontrolu (#AC)",
    "Makine kontrolu (#MC)",
    "SIMD kayan nokta (#XM)",
    "Sanallastirma (#VE)",
    "Kontrol korumasi (#CP)",
    "Ayrilmis",
    "Ayrilmis",
    "Ayrilmis",
    "Ayrilmis",
    "Ayrilmis",
    "Ayrilmis",
    "Ayrilmis",
    "Ayrilmis",
    "Ayrilmis",
    "Ayrilmis",
];

/// Sistem çağrısı (int 0x80) çerçevesi. `isr_syscall` stub'ı ile uyumlu.
/// Ayrıcalık değişimi olduğundan CPU eflags'tan sonra useresp/ss de iter.
#[repr(C)]
pub struct SyscallFrame {
    gs: u32,
    fs: u32,
    es: u32,
    ds: u32,
    edi: u32,
    esi: u32,
    ebp: u32,
    esp_dummy: u32,
    ebx: u32,
    edx: u32,
    ecx: u32,
    eax: u32,
    eip: u32,
    cs: u32,
    eflags: u32,
    useresp: u32,
    ss: u32,
}

/// `isr_syscall` buraya çağırır. Dönüş: 0 = ring3'e geri dön, 1 = çıkış
/// (çekirdek bağlamına dön).
/// Son kullanıcı programının çıkış kodu (SYS_EXIT ile).
static mut LAST_EXIT_CODE: u32 = 0;
/// Süreç (process) kimliği sayaçları.
static mut NEXT_PID: u32 = 1;
static mut CUR_PID: u32 = 0;

/// `user_exit_code()`: en son ring 3 programının döndürdüğü çıkış kodu.
pub fn user_exit_code() -> u32 {
    unsafe { LAST_EXIT_CODE }
}

/// Yeni bir süreç başlat: bir sonraki PID'i atar ve döndürür.
pub fn begin_process() -> u32 {
    unsafe {
        CUR_PID = NEXT_PID;
        NEXT_PID += 1;
        CUR_PID
    }
}

// Sistem çağrısı numaraları (kullanıcı programıyla ortak sözleşme).
const SYS_WRITE: u32 = 1; // write(fd, ptr, len)
const SYS_EXIT: u32 = 2; // exit(kod)
const SYS_READ: u32 = 3; // read(fd, ptr, len)
const SYS_GETPID: u32 = 4; // getpid()
const SYS_OPEN: u32 = 5; // open(path_ptr, path_len, flags)
const SYS_CLOSE: u32 = 6; // close(fd)
const SYS_SLEEP: u32 = 7; // sleep(ticks)
const SYS_GETARG: u32 = 13; // getarg(buf, cap)
const SYS_SBRK: u32 = 8; // sbrk(increment) -> eski brk / -1
const SYS_FETCH: u32 = 9; // fetch(url_ptr, out_ptr, out_cap) -> bayt / -1
const SYS_GMODE: u32 = 10; // grafik moda geç -> (genişlik<<16)|yükseklik / 0
const SYS_FILLRECT: u32 = 11; // fill_rect(xy, wh, rgb)
const SYS_BLIT: u32 = 12; // blit(buf, xy, wh)

// Standart akış tanıtıcıları.
const FD_STDIN: u32 = 0;
const FD_STDOUT: u32 = 1;

/// `isr_syscall` buraya çağırır. Dönüş: 0 = ring3'e geri dön, 1 = çıkış.
/// Sistem çağrısının kullanıcıya verdiği sonuç `frame.eax`'e yazılır (stub'taki
/// `popa` bunu user EAX'ine geri yükler). Unix benzeri sözleşme: ebx/ecx/edx
/// argümanlar, eax dönüş değeri (-1 = u32::MAX hata).
#[no_mangle]
pub extern "C" fn syscall_dispatch(frame: *mut SyscallFrame) -> u32 {
    let f = unsafe { &mut *frame };
    match f.eax {
        // write(ebx = fd, ecx = tampon, edx = uzunluk) -> eax = yazılan bayt.
        // fd 1 (stdout) konsola yazar; fd >= 3 açık bir dosyaya yazar.
        SYS_WRITE => {
            let (fd, ptr, len) = (f.ebx, f.ecx, f.edx);
            f.eax = if !mem::user_range_ok(ptr, len) {
                u32::MAX
            } else if fd == FD_STDOUT {
                vga::set_color(Color::White, Color::Black);
                for i in 0..len {
                    let c = unsafe { *((ptr + i) as *const u8) };
                    crate::print!("{}", c as char);
                }
                vga::set_color(Color::LightGray, Color::Black);
                len
            } else {
                let src = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
                crate::fd::write(fd, src)
            };
            0
        }
        // exit(ebx = çıkış kodu): çekirdeğe geri dön.
        SYS_EXIT => {
            unsafe { LAST_EXIT_CODE = f.ebx };
            serial::write_str("[syscall] SYS_EXIT kod=");
            serial::write_dec(f.ebx);
            serial::write_str("\n");
            1
        }
        // read(ebx = fd, ecx = tampon, edx = uzunluk) -> eax = okunan bayt.
        // fd 0 (stdin) klavyeden bir satır okur; fd >= 3 açık bir dosyadan okur.
        SYS_READ => {
            let (fd, ptr, len) = (f.ebx, f.ecx, f.edx);
            f.eax = if !mem::user_range_ok(ptr, len) {
                u32::MAX
            } else if fd == FD_STDIN {
                sys_read_line(ptr, len)
            } else {
                let dst = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, len as usize) };
                crate::fd::read(fd, dst)
            };
            0
        }
        // getpid() -> eax = geçerli sürecin PID'i.
        SYS_GETPID => {
            f.eax = unsafe { CUR_PID };
            0
        }
        // open(ebx = ad tamponu, ecx = ad uzunluğu, edx = bayraklar) -> eax = fd / -1.
        SYS_OPEN => {
            let (ptr, len, flags) = (f.ebx, f.ecx, f.edx);
            f.eax = if mem::user_range_ok(ptr, len) && len <= 28 {
                let name = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
                let parent = crate::shell::current_dir();
                crate::fd::open(name, parent, flags)
            } else {
                u32::MAX
            };
            0
        }
        // close(ebx = fd) -> eax = 0 / -1.
        SYS_CLOSE => {
            f.eax = crate::fd::close(f.ebx);
            0
        }
        // sleep(ebx = tik sayisi) -> eax = 0.
        SYS_SLEEP => {
            crate::task::sleep_ticks(f.ebx as u64);
            f.eax = 0;
            0
        }
        // sbrk(ebx = artış, işaretli) -> eax = eski brk (hata: -1).
        // Heap'i büyütür/küçültür; malloc bunu kullanır.
        SYS_SBRK => {
            f.eax = mem::user_sbrk(f.ebx as i32);
            0
        }
        // fetch(ebx = url (C-dizisi), ecx = çıktı tamponu, edx = kapasite)
        // -> eax = gövdeye yazılan bayt / -1. Çekirdeğin HTTP/HTTPS (rustls)
        // yığınını kullanıcı alanına açar — "userspace tarayıcı" temeli.
        SYS_FETCH => {
            f.eax = sys_fetch(f.ebx, f.ecx, f.edx);
            0
        }
        // gmode(): grafik moda geçer (gerekirse VBE'yi açar) -> (w<<16)|h / 0.
        SYS_GMODE => {
            f.eax = if crate::gfx::is_active()
                || crate::gfx::enable_bochs(1024, 768, 32)
            {
                crate::fbcon::init();
                ((crate::gfx::width() as u32) << 16) | (crate::gfx::height() as u32)
            } else {
                0
            };
            0
        }
        // fill_rect(ebx = (x<<16)|y, ecx = (w<<16)|h, edx = 0xRRGGBB).
        SYS_FILLRECT => {
            let (x, y) = ((f.ebx >> 16) as usize, (f.ebx & 0xFFFF) as usize);
            let (w, h) = ((f.ecx >> 16) as usize, (f.ecx & 0xFFFF) as usize);
            crate::gfx::fill_rect(x, y, w, h, crate::gfx::rgb(f.edx));
            f.eax = 0;
            0
        }
        // blit(ebx = tampon (0xRRGGBB[]), ecx = (x<<16)|y, edx = (w<<16)|h).
        SYS_BLIT => {
            let buf = f.ebx;
            let (x, y) = ((f.ecx >> 16) as usize, (f.ecx & 0xFFFF) as usize);
            let (w, h) = ((f.edx >> 16) as usize, (f.edx & 0xFFFF) as usize);
            let bytes = (w * h * 4) as u32;
            f.eax = if mem::user_range_ok(buf, bytes) {
                crate::gfx::blit_rgb(x, y, w, h, buf as *const u32);
                0
            } else {
                u32::MAX
            };
            0
        }
        // getarg(ebx = tampon, ecx = kapasite) -> eax = kopyalanan bayt sayısı.
        // `run <prog> <arg...>` argüman dizesini kullanıcı alanına verir (argv).
        SYS_GETARG => {
            let (ptr, cap) = (f.ebx, f.ecx);
            f.eax = if cap > 0 && mem::user_range_ok(ptr, cap) {
                let dst = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, cap as usize) };
                crate::shell::run_arg_copy(dst)
            } else {
                0
            };
            0
        }
        other => {
            crate::println!("  [ring3] bilinmeyen sistem cagrisi: eax={other}");
            f.eax = u32::MAX; // -1
            0
        }
    }
}

/// Klavyeden bir satır okuyup kullanıcı tamponuna (UTF-8) yazar. Enter'da biter,
/// Backspace son baytı siler. Okunan bayt sayısını döndürür.
fn sys_read_line(ptr: u32, max: u32) -> u32 {
    let mut written: u32 = 0;
    loop {
        let c = crate::keyboard::read_char();
        match c {
            '\n' => {
                crate::print!("\n");
                break;
            }
            '\u{8}' => {
                if written > 0 {
                    written -= 1;
                    crate::print!("\u{8}");
                }
            }
            crate::keyboard::KEY_TOGGLE | crate::keyboard::KEY_ESC => {}
            // Yön/düzenleme tuşları (PUA E010+) — satır okuyucuda yok say.
            c if (c as u32) >= 0xE000 => {}
            c if !c.is_control() => {
                let mut tmp = [0u8; 4];
                let bytes = c.encode_utf8(&mut tmp).as_bytes();
                if written as usize + bytes.len() <= max as usize {
                    for &b in bytes {
                        unsafe { *((ptr + written) as *mut u8) = b };
                        written += 1;
                    }
                    crate::print!("{c}");
                }
            }
            _ => {}
        }
    }
    written
}

/// `SYS_FETCH`: kullanıcıdan gelen URL'yi (C-dizisi) çekip gövdeyi kullanıcı
/// tamponuna kopyalar. Çekirdeğin HTTP/HTTPS yığınını çalıştırır.
///
/// ÖNEMLİ: Ağ yığını zaman aşımları için PIT (IRQ0) zamanlayıcısının ilerlemesi
/// gerekir; ama `int 0x80` bir kesme kapısıdır (IF=0). Bu yüzden bu uzun, bloklayan
/// işlem boyunca kesmeleri AÇARIZ (`sti`), sonunda tekrar kapatırız (`cli`).
fn sys_fetch(url_ptr: u32, out_ptr: u32, out_cap: u32) -> u32 {
    // URL'yi (null-sonlu) sınırlı şekilde oku.
    let mut buf = [0u8; 512];
    let mut n = 0usize;
    while n < buf.len() - 1 {
        if !mem::user_range_ok(url_ptr + n as u32, 1) {
            return u32::MAX;
        }
        let c = unsafe { *((url_ptr + n as u32) as *const u8) };
        if c == 0 {
            break;
        }
        buf[n] = c;
        n += 1;
    }
    let url = match core::str::from_utf8(&buf[..n]) {
        Ok(s) => s,
        Err(_) => return u32::MAX,
    };
    if out_cap == 0 || !mem::user_range_ok(out_ptr, out_cap) {
        return u32::MAX;
    }

    // Zamanlayıcı tiklesin diye kesmeleri aç.
    unsafe { core::arch::asm!("sti", options(nomem, nostack)) };
    let result = crate::net::fetch_url(url);
    unsafe { core::arch::asm!("cli", options(nomem, nostack)) };

    match result {
        Ok(body) => {
            let bytes = body.as_bytes();
            let len = core::cmp::min(bytes.len(), out_cap as usize);
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr as *mut u8, len);
            }
            len as u32
        }
        Err(e) => {
            crate::println!("  [fetch] hata: {e}");
            u32::MAX
        }
    }
}

fn read_cr2() -> u32 {
    let v: u32;
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) v, options(nomem, nostack, preserves_flags));
    }
    v
}

/// isr.asm'deki ortak stub buraya çağırır. Tüm CPU istisnaları buradan geçer.
#[no_mangle]
pub extern "C" fn isr_common_handler(frame: *mut IsrFrame) {
    let f = unsafe { &*frame };
    let n = f.int_num;

    // #BP (breakpoint): bilgi ver ve sorunsuz dön (iret) — IDT/iret'in
    // çalıştığını kanıtlayan zararsız test yolu.
    if n == 3 {
        let eip = f.eip;
        serial::write_str("[idt] #BP breakpoint yakalandi @ eip=");
        serial::write_hex(eip);
        serial::write_str(", donuluyor\n");
        return;
    }

    panic_screen(f);
}

fn panic_screen(f: &IsrFrame) -> ! {
    let n = f.int_num;
    let eip = f.eip;
    let err = f.err_code;
    let cs = f.cs;
    let eflags = f.eflags;
    let name = NAMES.get(n as usize).copied().unwrap_or("Bilinmeyen");

    // Seri porta (başsız hata ayıklama için) dök.
    serial::write_str("\n*** CPU ISTISNASI ***\n  no=");
    serial::write_dec(n);
    serial::write_str("  eip=");
    serial::write_hex(eip);
    serial::write_str("  err=");
    serial::write_hex(err);
    if n == 14 {
        serial::write_str("  cr2=");
        serial::write_hex(read_cr2());
    }
    serial::write_str("\n");

    // Ekrana panik bilgisini bas.
    vga::set_color(Color::White, Color::Red);
    vga::clear();
    crate::println!(" *** CPU ISTISNASI / KERNEL PANIC ***");
    crate::println!();
    vga::set_color(Color::Yellow, Color::Black);
    crate::println!(" #{n}  {name}");
    vga::set_color(Color::LightGray, Color::Black);
    crate::println!();
    crate::println!(" EIP    : {eip:#010x}");
    crate::println!(" CS     : {cs:#06x}");
    crate::println!(" EFLAGS : {eflags:#010x}");
    crate::println!(" HATA   : {err:#010x}");

    if n == 14 {
        let cr2 = read_cr2();
        crate::println!(" CR2    : {cr2:#010x}   <- hatali erisim adresi");
        crate::println!(
            "   neden : {} / {} / {}",
            if err & 1 != 0 {
                "koruma ihlali"
            } else {
                "sayfa yok"
            },
            if err & 2 != 0 { "yazma" } else { "okuma" },
            if err & 4 != 0 {
                "kullanici modu"
            } else {
                "cekirdek modu"
            }
        );
    }

    crate::println!();
    vga::set_color(Color::LightRed, Color::Black);
    crate::println!(" Sistem durduruldu. Yeniden baslatmak icin makineyi kapatip acin.");

    loop {
        unsafe {
            core::arch::asm!("cli; hlt", options(nomem, nostack, preserves_flags));
        }
    }
}
