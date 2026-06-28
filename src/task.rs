//! Faz 4 — İşbirlikçi (cooperative) çekirdek görevleri + bağlam değiştirme.
//!
//! Birden fazla "görev" (task) aynı çekirdek içinde, her biri kendi yığınına
//! (stack) sahip olacak şekilde sırayla (round-robin) çalıştırılır. Bir görev
//! `yield_now()` çağırınca CPU bir sonraki çalışabilir göreve geçer; bu, küçük
//! bir assembly rutiniyle (`context_switch`) yazmaçları ve yığını değiştirerek
//! yapılır. Görevler arasında geçiş "işbirlikçidir": her görev gönüllü olarak
//! sırayı bırakır. (Zamanlayıcı kesmesiyle ZORUNLU/önleyici geçiş bir sonraki
//! adımdır; altyapı — IRQ0 — zaten hazır.)
//!
//! Görev 0 = "ana" bağlam (kabuk). Çalışabilir görev kalmayınca kontrol ona
//! döner ve `run_demo` çağıranına geri verir.

use crate::println;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const MAX_TASKS: usize = 8;
const STACK_SIZE: usize = 16 * 1024;

/// Önleyici (preemptive) zamanlama açık mı? Açıkken IRQ0 (zamanlayıcı) her tikte
/// görevleri zorla değiştirir.
static PREEMPT: AtomicBool = AtomicBool::new(false);
/// Önleyici demo görevlerinin yineleme sayaçları.
static COUNTS: [AtomicU64; MAX_TASKS] = [const { AtomicU64::new(0) }; MAX_TASKS];
/// Önleyici demo süresi (tik cinsinden).
static DURATION: AtomicU64 = AtomicU64::new(0);

extern "C" {
    fn context_switch(old_esp: *mut u32, new_esp: u32);
}

#[derive(Clone, Copy)]
struct Task {
    esp: u32,
    entry: usize,
    used: bool,
    finished: bool,
    /// 0 = uyanık; >0 ise bu tikten önce çalıştırılmaz.
    sleep_until: u64,
}

impl Task {
    const EMPTY: Task = Task {
        esp: 0,
        entry: 0,
        used: false,
        finished: false,
        sleep_until: 0,
    };
}

static mut TASKS: [Task; MAX_TASKS] = [Task::EMPTY; MAX_TASKS];
static mut STACKS: [[u8; STACK_SIZE]; MAX_TASKS] = [[0; STACK_SIZE]; MAX_TASKS];
static mut NTASKS: usize = 0;
static mut CURRENT: usize = 0;

/// Çalışmakta olan görevin kimliği (0 = ana bağlam).
pub fn current_id() -> usize {
    unsafe { CURRENT }
}

fn tasks() -> &'static mut [Task; MAX_TASKS] {
    unsafe { &mut *core::ptr::addr_of_mut!(TASKS) }
}

fn runnable(id: usize) -> bool {
    let t = &tasks()[id];
    if !t.used || t.finished {
        return false;
    }
    let now = crate::pic::ticks();
    t.sleep_until == 0 || now >= t.sleep_until
}

/// `cur`'dan sonraki çalışabilir görevi (round-robin) seçer; yoksa ana (0).
fn pick_next(cur: usize) -> usize {
    let n = unsafe { NTASKS };
    let t = tasks();
    for off in 1..=n {
        let cand = (cur + off) % n;
        if cand != 0 && runnable(cand) {
            // Uyku süresi dolduysa bayrağı temizle.
            if t[cand].sleep_until > 0 && crate::pic::ticks() >= t[cand].sleep_until {
                t[cand].sleep_until = 0;
            }
            return cand;
        }
    }
    0
}

/// Geçerli görevi `ticks` kadar uyut (önleyici veya işbirlikçi).
pub fn sleep_ticks(ticks: u64) {
    let id = current_id();
    if id == 0 || ticks == 0 {
        return;
    }
    let wake = crate::pic::ticks() + ticks;
    tasks()[id].sleep_until = wake;
    while crate::pic::ticks() < wake {
        yield_now();
    }
    tasks()[id].sleep_until = 0;
}

/// Geçerli görev gönüllü olarak sırayı bırakır; bir sonraki göreve geçilir.
pub fn yield_now() {
    unsafe {
        let cur = CURRENT;
        let next = pick_next(cur);
        if next == cur {
            return;
        }
        CURRENT = next;
        let t = tasks();
        let old = core::ptr::addr_of_mut!(t[cur].esp);
        let new = t[next].esp;
        context_switch(old, new);
    }
}

/// IRQ0 (zamanlayıcı) her tikte buraya uğrar. Önleyici zamanlama açıksa geçerli
/// görevi durdurup bir sonrakine geçer. KESME BAĞLAMINDA çalışır (IF=0); EOI
/// `pic` tarafından bu çağrıdan ÖNCE gönderilmiştir.
pub fn on_tick() {
    if !PREEMPT.load(Ordering::Relaxed) {
        return;
    }
    unsafe {
        let cur = CURRENT;
        let next = pick_next(cur);
        if next == cur {
            return;
        }
        CURRENT = next;
        let t = tasks();
        let old = core::ptr::addr_of_mut!(t[cur].esp);
        let new = t[next].esp;
        context_switch(old, new);
    }
}

/// Yeni görevler bu ara fonksiyonda başlar: gerçek giriş fonksiyonunu çağırır,
/// dönünce görevi "bitti" işaretler ve sırayı bırakır (geri dönmez).
///
/// `sti`: yeni bir görev ilk kez `context_switch` ile (cooperative) veya IRQ
/// bağlamında (preemptive, IF=0) başlayabilir; her durumda kesmeleri açık
/// tutmak için `sti` veririz — aksi halde önleyici zamanlama duraksardı.
extern "C" fn trampoline() -> ! {
    unsafe { core::arch::asm!("sti", options(nomem, nostack)) };
    let entry = tasks()[current_id()].entry;
    let f: fn() = unsafe { core::mem::transmute::<usize, fn()>(entry) };
    f();
    tasks()[current_id()].finished = true;
    loop {
        yield_now();
    }
}

/// Yeni bir görev oluşturur: yığınını, ilk `context_switch` dönüşünde
/// `trampoline`'a "ret" edecek şekilde hazırlar.
fn spawn(entry: fn()) {
    unsafe {
        let i = NTASKS;
        if i >= MAX_TASKS {
            return;
        }
        let stack = &mut *core::ptr::addr_of_mut!(STACKS[i]);
        let top = stack.as_mut_ptr().add(STACK_SIZE) as *mut u32;
        // context_switch: pop ebp; pop edi; pop esi; pop ebx; ret
        // Yığın (yüksekten alçağa): [ret=trampoline][ebx][esi][edi][ebp]
        let mut sp = top;
        sp = sp.sub(1);
        *sp = trampoline as *const () as u32; // ret
        sp = sp.sub(1);
        *sp = 0; // ebx
        sp = sp.sub(1);
        *sp = 0; // esi
        sp = sp.sub(1);
        *sp = 0; // edi
        sp = sp.sub(1);
        *sp = 0; // ebp
        tasks()[i] = Task {
            esp: sp as u32,
            entry: entry as usize,
            used: true,
            finished: false,
            sleep_until: 0,
        };
        NTASKS = i + 1;
    }
}

/// `workers` adet örnek görev oluşturup round-robin çalıştırır; hepsi bitince
/// kabuğa döner.
pub fn run_demo(workers: usize) {
    unsafe {
        // Görev 0 = ana bağlam (kabuk). ESP'si ilk geçişte kaydedilir.
        tasks()[0] = Task {
            esp: 0,
            entry: 0,
            used: true,
            finished: false,
            sleep_until: 0,
        };
        NTASKS = 1;
        CURRENT = 0;
    }

    let n = workers.min(MAX_TASKS - 1);
    for _ in 0..n {
        spawn(worker);
    }

    // İlk çalışabilir görevciye geç.
    let first = pick_next(0);
    if first != 0 {
        unsafe {
            CURRENT = first;
            let t = tasks();
            let old = core::ptr::addr_of_mut!(t[0].esp);
            let new = t[first].esp;
            context_switch(old, new);
        }
    }
    // Buraya tüm görevler bitince (kontrol ana bağlama dönünce) gelinir.
}

/// Örnek görev: kimliğini birkaç tur yazıp her turda sırayı bırakır.
fn worker() {
    let id = current_id();
    for round in 0..5 {
        println!("  [gorev {id}] tur {round} calisiyor");
        yield_now();
    }
    println!("  [gorev {id}] bitti");
}

/// `workers` adet hesap-yoğun görevi ÖNLEYİCİ (timer ile zorla) zamanlamayla
/// `dur` tik boyunca çalıştırır. Görevler `yield` ÇAĞIRMAZ; sadece sayaç artırır.
/// Sonunda her görevin kaç yineleme yaptığını yazar — zaman paylaşımının kanıtı.
pub fn run_preempt(workers: usize, dur: u64) {
    unsafe {
        tasks()[0] = Task {
            esp: 0,
            entry: 0,
            used: true,
            finished: false,
            sleep_until: 0,
        };
        NTASKS = 1;
        CURRENT = 0;
    }
    for c in COUNTS.iter() {
        c.store(0, Ordering::Relaxed);
    }
    DURATION.store(dur, Ordering::Relaxed);

    let n = workers.min(MAX_TASKS - 1);
    for _ in 0..n {
        spawn(pworker);
    }

    // Önleyici zamanlamayı aç; ana bağlam tüm görevler bitene kadar bekler.
    PREEMPT.store(true, Ordering::Relaxed);
    loop {
        let mut all_done = true;
        let t = tasks();
        for i in 1..unsafe { NTASKS } {
            if t[i].used && !t[i].finished {
                all_done = false;
                break;
            }
        }
        if all_done {
            break;
        }
        // Bir sonraki kesmeyi bekle (CPU'yu boşa harcama); timer bizi görevlere
        // geçirir, hepsi bitince buraya döneriz.
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)) };
    }
    PREEMPT.store(false, Ordering::Relaxed);

    for i in 1..unsafe { NTASKS } {
        println!(
            "  [gorev {i}] {} yineleme yapti",
            COUNTS[i].load(Ordering::Relaxed)
        );
    }
}

/// Önleyici demo görevi: `yield` çağırmadan, süre dolana dek sayaç artırır.
/// Timer kesmesi onu zorla bölüp diğer görevlere geçirir.
fn pworker() {
    let id = current_id();
    let start = crate::pic::ticks();
    let dur = DURATION.load(Ordering::Relaxed);
    while crate::pic::ticks() < start + dur {
        COUNTS[id].fetch_add(1, Ordering::Relaxed);
        core::hint::spin_loop();
    }
}

/// Uyku (sleep) sınaması: görevler farklı süreler uyur, uyanınca mesaj basar.
pub fn run_sleep_demo() {
    unsafe {
        tasks()[0] = Task {
            esp: 0,
            entry: 0,
            used: true,
            finished: false,
            sleep_until: 0,
        };
        NTASKS = 1;
        CURRENT = 0;
    }
    spawn(sleeper_a);
    spawn(sleeper_b);
    spawn(sleeper_c);

    PREEMPT.store(true, Ordering::Relaxed);
    let first = pick_next(0);
    if first != 0 {
        unsafe {
            CURRENT = first;
            let t = tasks();
            context_switch(core::ptr::addr_of_mut!(t[0].esp), t[first].esp);
        }
    }
    PREEMPT.store(false, Ordering::Relaxed);
}

fn sleeper_a() {
    println!("  [A] basladi, 30 tik uyuyacak");
    sleep_ticks(30);
    println!("  [A] uyandi (30 tik sonra)");
}

fn sleeper_b() {
    println!("  [B] basladi, 60 tik uyuyacak");
    sleep_ticks(60);
    println!("  [B] uyandi (60 tik sonra)");
}

fn sleeper_c() {
    println!("  [C] basladi, hemen calisir");
    sleep_ticks(5);
    println!("  [C] kisa uyku bitti");
}
