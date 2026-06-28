//! Faz 1 — Fiziksel bellek yönetimi ve sayfalama (paging).
//!
//! Bu modül çekirdeğin ilk adımlarından biri olarak çalışır (heap'ten de önce):
//!
//! 1. **Sayfalama**: Tüm 4 GiB adres alanını 4 MiB'lik büyük sayfalarla *birebir*
//!    (identity) eşler ve sayfalamayı açar. Birebir eşleme sayesinde sanal adres =
//!    fiziksel adres olur; mevcut hiçbir kod/sürücü değişmeden çalışmaya devam
//!    eder. MMIO bölgeleri (>= 0xC000_0000: PCI pencereleri, framebuffer, aygıt
//!    register'ları) önbelleksiz (PCD/PWT) işaretlenir ki gerçek donanımda da
//!    doğru çalışsın.
//!
//! 2. **RAM tespiti**: Multiboot bellek haritasını (mmap) okuyup gerçekten
//!    kullanılabilir en büyük RAM bölgesini bulur.
//!
//! 3. **Yığın (heap)**: Çekirdek imajının bittiği yerden itibaren bu RAM'in büyük
//!    bölümünü global ayırıcıya verir (artık 4 MiB değil, RAM kadar). Üstte kalan
//!    bir dilimi ileride kullanıcı alanı/sayfa tabloları için çerçeve (frame)
//!    ayırıcısına bırakır.
//!
//! Not: Bu aşamada henüz heap kurulmadığı için burada `alloc` (Vec/String/format!)
//! KULLANILMAZ; günlükleme tampon temelli, ayırma yapmayan yardımcılarla yapılır.

use crate::serial;

extern "C" {
    /// Linker (linker.ld) tarafından çekirdek imajının sonuna yerleştirilir.
    static kernel_end: u8;
}

// --- Sayfalama tablosu (page directory) ---

#[repr(C, align(4096))]
struct PageDir([u32; 1024]);

static mut PAGE_DIR: PageDir = PageDir([0; 1024]);

const PRESENT: u32 = 1 << 0;
const RW: u32 = 1 << 1;
const PWT: u32 = 1 << 3; // write-through
const PCD: u32 = 1 << 4; // önbellek kapalı (cache disable)
const PS: u32 = 1 << 7; // 4 MiB sayfa (PSE)

const FOUR_MIB: u32 = 0x0040_0000;
/// Bu adresin üstü tipik 32-bit PCI MMIO penceresidir; önbelleksiz eşlenir.
const MMIO_BASE: u32 = 0xC000_0000;

// --- Çerçeve (frame) ayırıcı ---

const FRAME_SIZE: usize = 4096;
/// Çerçeve ayırıcıya en fazla 32 MiB ayrılır → 8192 çerçeve → 1 KiB bit eşlemi.
const MAX_FRAMES: usize = (32 * 1024 * 1024) / FRAME_SIZE;

static mut FRAME_BITMAP: [u8; MAX_FRAMES / 8] = [0; MAX_FRAMES / 8];
static mut FRAME_BASE: usize = 0;
static mut FRAME_COUNT: usize = 0;

/// Tespit edilen toplam kullanılabilir RAM (bayt) — `mem` komutu için saklanır.
static mut TOTAL_RAM: usize = 0;

// --- Eşleme yardımcıları (ayırma yapmaz) ---

fn rd_u32(addr: usize) -> u32 {
    unsafe { core::ptr::read_unaligned(addr as *const u32) }
}

fn rd_u64(addr: usize) -> u64 {
    (rd_u32(addr) as u64) | ((rd_u32(addr + 4) as u64) << 32)
}

fn align_up(v: usize, a: usize) -> usize {
    (v + a - 1) & !(a - 1)
}

/// Sayfalamayı açar, RAM'i tespit eder, çerçeve ayırıcıyı kurar ve
/// global yığın (heap) için kullanılacak `(başlangıç, boyut)` döndürür.
pub fn init(mbi: u32) -> (usize, usize) {
    enable_paging();

    let kend = core::ptr::addr_of!(kernel_end) as usize;
    let (region_base, region_end, total) = detect_ram(mbi, kend);
    unsafe {
        TOTAL_RAM = total;
    }

    // Heap, çekirdek imajından (veya bölge başından) sonra başlar.
    let heap_start = align_up(core::cmp::max(region_base, kend), 0x1000);
    let avail = region_end.saturating_sub(heap_start);

    // Üstten bir miktarı çerçeve ayırıcıya ayır (gelecekteki sayfa tabloları /
    // kullanıcı sayfaları için), gerisini heap'e ver. Heap boyutunu sayfaya
    // hizalarız ki çerçeve havuzu tabanı (ve dolayısıyla her çerçeve) 4 KiB
    // hizalı olsun — sayfa tablosu girdileri için ŞART.
    let reserve = core::cmp::min(avail / 8, MAX_FRAMES * FRAME_SIZE);
    let heap_size = (avail - reserve) & !0xFFF;
    let frame_base = heap_start + heap_size;

    frame_init(frame_base, reserve);

    serial::write_str("[mem] sayfalama acik (identity, 4 MiB sayfa)\n");
    log_num("[mem] toplam RAM: ", total / (1024 * 1024), " MiB\n");
    log_hex("[mem] heap @ ", heap_start);
    log_num(" boyut: ", heap_size / (1024 * 1024), " MiB\n");
    log_num("[mem] cerceve (frame) havuzu: ", reserve / (1024 * 1024), " MiB\n");

    (heap_start, heap_size)
}

/// 4 GiB'lik adres alanını 4 MiB sayfalarla birebir eşleyip sayfalamayı açar.
fn enable_paging() {
    unsafe {
        let pd = &mut *core::ptr::addr_of_mut!(PAGE_DIR);
        let mut i = 0usize;
        while i < 1024 {
            let base = (i as u32).wrapping_mul(FOUR_MIB);
            let mut entry = base | PRESENT | RW | PS;
            if base >= MMIO_BASE {
                entry |= PCD | PWT; // MMIO: önbelleksiz
            }
            pd.0[i] = entry;
            i += 1;
        }

        let pd_addr = pd.0.as_ptr() as u32;
        core::arch::asm!(
            "mov cr3, {pd}",
            "mov {t}, cr4",
            "or {t}, 0x10",        // CR4.PSE = 1 (4 MiB sayfalar)
            "mov cr4, {t}",
            "mov {t}, cr0",
            "or {t}, 0x80000000",  // CR0.PG = 1 (sayfalamayı aç)
            "mov cr0, {t}",
            pd = in(reg) pd_addr,
            t = out(reg) _,
            options(nostack),
        );
    }
}

/// Multiboot bellek haritasından en büyük kullanılabilir RAM bölgesini bulur.
/// Dönüş: `(bolge_baslangic, bolge_bitis, toplam_kullanilabilir)`.
fn detect_ram(mbi: u32, _kend: usize) -> (usize, usize, usize) {
    // mmap yoksa: mem_upper (1 MiB üstündeki KiB) ya da güvenli 32 MiB varsayımı.
    let fallback = |mbi: u32| -> (usize, usize, usize) {
        let mem_upper = if mbi != 0 {
            rd_u32(mbi as usize + 8) as usize
        } else {
            0
        };
        let end = if mem_upper > 0 {
            0x10_0000 + mem_upper * 1024
        } else {
            0x200_0000 // 32 MiB
        };
        (0x10_0000, end, end - 0x10_0000)
    };

    if mbi == 0 {
        return fallback(mbi);
    }
    let flags = rd_u32(mbi as usize);
    if flags & (1 << 6) == 0 {
        return fallback(mbi); // mmap geçersiz
    }

    let mmap_len = rd_u32(mbi as usize + 44) as usize;
    let mmap_addr = rd_u32(mbi as usize + 48) as usize;

    let mut best_base = 0usize;
    let mut best_len = 0usize;
    let mut total = 0usize;

    let mut p = mmap_addr;
    let end_p = mmap_addr + mmap_len;
    while p < end_p {
        let size = rd_u32(p) as usize; // bu alanı saymayan giriş boyutu
        let base = rd_u64(p + 4);
        let len = rd_u64(p + 12);
        let typ = rd_u32(p + 20);

        if typ == 1 && base < 0xFFFF_FFFF {
            let b = base as usize;
            // 4 GiB üstünü (sayfalama 32-bit) yok say.
            let l = if base + len > 0xFFFF_FFFF {
                (0xFFFF_FFFF - base) as usize
            } else {
                len as usize
            };
            total += l;
            if l > best_len {
                best_len = l;
                best_base = b;
            }
        }

        if size == 0 {
            break;
        }
        p += size + 4;
    }

    if best_len == 0 {
        return fallback(mbi);
    }

    let mut region_end = best_base + best_len;
    // Heap'i MMIO penceresinin altında tut.
    if region_end > MMIO_BASE as usize {
        region_end = MMIO_BASE as usize;
    }
    (best_base, region_end, total)
}

// --- Çerçeve (frame) ayırıcı: 4 KiB fiziksel çerçeveler, bit eşlemli ---

fn frame_init(base: usize, len: usize) {
    // Çerçeveler 4 KiB hizalı olmalı (sayfa tablosu girdileri için).
    let base = align_up(base, FRAME_SIZE);
    let len = len.saturating_sub(base - (base & !0xFFF));
    let count = core::cmp::min(len / FRAME_SIZE, MAX_FRAMES);
    unsafe {
        FRAME_BASE = base;
        FRAME_COUNT = count;
        let bm = &mut *core::ptr::addr_of_mut!(FRAME_BITMAP);
        for b in bm.iter_mut() {
            *b = 0;
        }
    }
}

/// Boş bir 4 KiB fiziksel çerçeve ayırır (birebir eşli olduğundan sanal=fiziksel).
#[allow(dead_code)]
pub fn alloc_frame() -> Option<usize> {
    unsafe {
        let bm = &mut *core::ptr::addr_of_mut!(FRAME_BITMAP);
        for i in 0..FRAME_COUNT {
            let (byte, bit) = (i / 8, i % 8);
            if bm[byte] & (1 << bit) == 0 {
                bm[byte] |= 1 << bit;
                return Some(FRAME_BASE + i * FRAME_SIZE);
            }
        }
        None
    }
}

/// `alloc_frame` ile alınmış bir çerçeveyi geri verir.
#[allow(dead_code)]
pub fn free_frame(addr: usize) {
    unsafe {
        if addr < FRAME_BASE {
            return;
        }
        let i = (addr - FRAME_BASE) / FRAME_SIZE;
        if i < FRAME_COUNT {
            let bm = &mut *core::ptr::addr_of_mut!(FRAME_BITMAP);
            bm[i / 8] &= !(1 << (i % 8));
        }
    }
}

/// Çerçeve havuzundaki boş çerçeve sayısı.
#[allow(dead_code)]
pub fn frames_free() -> usize {
    unsafe {
        let bm = &*core::ptr::addr_of!(FRAME_BITMAP);
        let mut c = 0;
        for i in 0..FRAME_COUNT {
            if bm[i / 8] & (1 << (i % 8)) == 0 {
                c += 1;
            }
        }
        c
    }
}

/// Tespit edilen toplam kullanılabilir RAM (bayt).
#[allow(dead_code)]
pub fn total_ram() -> usize {
    unsafe { TOTAL_RAM }
}

// --- Kullanıcı alanı (ring 3) için 4 KiB sayfa eşlemesi ---
//
// Sayfalama 4 MiB büyük sayfalarla kurulu (her PDE bir 4 MiB sayfa). Kullanıcıya
// ait sayfalarda US (user/supervisor) bitini ayarlayabilmek için tek bir PDE'yi
// 4 KiB'lik bir sayfa tablosuna çeviririz. Seçilen sanal taban (0x4000_0000 = 1
// GiB) fiziksel RAM'in (≤256 MiB) çok üstündedir; yani normalde kullanılmayan,
// güvenle yeniden amaçlanabilen bir adrestir.

/// Kullanıcı adres alanının sanal tabanı (1 GiB). Fiziksel RAM'in (≤256 MiB)
/// çok üstünde; tek bir 4 MiB'lik PDE bölgesi (4 KiB sayfa tablosu) kullanırız.
pub const USER_BASE: u32 = 0x4000_0000;
const USER_PDE_IDX: usize = (USER_BASE >> 22) as usize; // 256

/// Kullanıcı sanal bölgesi: 64 MiB = 16 adet 4 MiB'lik PDE penceresi (her biri
/// bir 4 KiB sayfa tablosu). Bu, NetSurf gibi gerçek C programlarının ihtiyaç
/// duyduğu büyüyebilen heap'e (`sbrk`) yer açar. Sayfalar talep üzerine eşlenir.
const USER_PDE_COUNT: usize = 16;
const USER_REGION_END: u32 = USER_BASE + (USER_PDE_COUNT as u32) * FOUR_MIB; // 64 MiB

const PTE_P: u32 = 1 << 0;
const PTE_RW: u32 = 1 << 1;
const PTE_US: u32 = 1 << 2;

// Kullanıcı yığını: bölgenin üst kısmında 64 sayfa (256 KiB).
const USER_STACK_TOP: u32 = USER_REGION_END - 0x1000; // son sayfayı koru
const USER_STACK_PAGES: u32 = 64;
const USER_STACK_BOTTOM: u32 = USER_STACK_TOP - USER_STACK_PAGES * 0x1000;

/// Bir ring 3 sürecine ait adres alanı: 16 adet 4 KiB sayfa tablosu (her biri bir
/// 4 MiB PDE penceresini kaplar) + büyüyebilen heap (`brk`). Çerçeveler ayrı bir
/// dizide değil, yıkımda sayfa tabloları gezilerek serbest bırakılır.
pub struct UserSpace {
    pts: [usize; USER_PDE_COUNT], // PDE başına sayfa tablosu çerçevesi (0 = yok)
    brk: u32,                     // güncel heap tepesi
    brk_start: u32,               // heap başlangıcı (ELF yüklemesinden sonra)
}

impl UserSpace {
    pub const EMPTY: Self = Self {
        pts: [0; USER_PDE_COUNT],
        brk: 0,
        brk_start: 0,
    };

    /// Yeni boş kullanıcı adres alanı (sayfa tabloları talep üzerine ayrılır).
    pub fn new() -> Self {
        Self::EMPTY
    }

    /// `vaddr`'ı içeren PDE penceresinin sayfa tablosunu gerekirse ayırır.
    fn ensure_pt(&mut self, vaddr: u32) -> usize {
        let rel = ((vaddr - USER_BASE) >> 22) as usize;
        if self.pts[rel] == 0 {
            let pt = alloc_frame().expect("cerceve havuzu bos (PT)");
            unsafe {
                core::ptr::write_bytes(pt as *mut u8, 0, FRAME_SIZE);
            }
            self.pts[rel] = pt;
        }
        self.pts[rel]
    }

    /// `vaddr`'ı içeren 4 KiB sayfayı eşler (US=1). İdempotsenttir.
    pub fn map_page(&mut self, vaddr: u32, writable: bool) -> usize {
        let pt = self.ensure_pt(vaddr);
        unsafe {
            let idx = ((vaddr >> 12) & 0x3FF) as usize;
            let ptp = pt as *mut u32;
            let existing = *ptp.add(idx);
            if existing & PTE_P != 0 {
                return (existing & !0xFFF) as usize;
            }
            let frame = alloc_frame().expect("cerceve havuzu bos (sayfa)");
            core::ptr::write_bytes(frame as *mut u8, 0, FRAME_SIZE);
            let mut flags = PTE_P | PTE_US;
            if writable {
                flags |= PTE_RW;
            }
            *ptp.add(idx) = (frame as u32) | flags;
            frame
        }
    }

    /// Kullanıcı yığınını eşler; yığın tepesini döndürür.
    pub fn setup_stack(&mut self) -> u32 {
        let mut p = USER_STACK_BOTTOM;
        while p < USER_STACK_TOP {
            self.map_page(p, true);
            p += 0x1000;
        }
        USER_STACK_TOP - 16
    }

    /// Heap başlangıcını/brk'ı ayarlar (ELF yüklemesinden sonra çağrılır).
    pub fn set_brk(&mut self, addr: u32) {
        let a = (addr + 0xFFF) & !0xFFF;
        self.brk = a;
        self.brk_start = a;
    }

    /// `sbrk`: heap'i büyütür/küçültür. Eski brk değerini döndürür; hata: `u32::MAX`.
    pub fn sbrk(&mut self, inc: i32) -> u32 {
        let old = self.brk;
        if inc == 0 {
            return old;
        }
        let new = (old as i64 + inc as i64) as u32;
        if new < self.brk_start || new > USER_STACK_BOTTOM {
            return u32::MAX;
        }
        if inc > 0 {
            let mut page = old & !0xFFF;
            while page < new {
                self.map_page(page, true);
                page += 0x1000;
            }
            // Büyüme yeni bir PDE penceresi açmış olabilir; PDE'leri tazele.
            self.activate();
        }
        self.brk = new;
        old
    }

    /// Tampon aralığı bu adres alanında mı?
    pub fn range_ok(&self, ptr: u32, len: u32) -> bool {
        let end = ptr as u64 + len as u64;
        ptr >= USER_BASE && end <= USER_REGION_END as u64
    }

    /// Bu adres alanını etkinleştirir (tüm PDE pencereleri + TLB).
    pub fn activate(&self) {
        unsafe {
            let pd = &mut *core::ptr::addr_of_mut!(PAGE_DIR);
            for i in 0..USER_PDE_COUNT {
                pd.0[USER_PDE_IDX + i] = if self.pts[i] != 0 {
                    (self.pts[i] as u32) | PTE_P | PTE_RW | PTE_US
                } else {
                    0
                };
            }
            flush_tlb();
        }
    }

    /// Sanal adresteki baytı okur (alan etkin olmalı).
    pub fn read_u32(&self, vaddr: u32) -> u32 {
        unsafe { core::ptr::read_unaligned(vaddr as *const u32) }
    }

    /// Tüm sayfa tablolarını gezip kullanıcı çerçevelerini ve tabloları bırakır.
    pub fn destroy(&mut self) {
        for i in 0..USER_PDE_COUNT {
            let pt = self.pts[i];
            if pt == 0 {
                continue;
            }
            unsafe {
                let ptp = pt as *const u32;
                for j in 0..1024usize {
                    let e = *ptp.add(j);
                    if e & PTE_P != 0 {
                        free_frame((e & !0xFFF) as usize);
                    }
                }
            }
            free_frame(pt);
            self.pts[i] = 0;
        }
        self.brk = 0;
        self.brk_start = 0;
    }
}

// Tek süreç API'si (elf/run) için geçerli aktif alan.
static mut LEGACY: UserSpace = UserSpace::EMPTY;
static mut SAVED_PDE: [u32; USER_PDE_COUNT] = [0; USER_PDE_COUNT];

/// `LEGACY` statiğine ham işaretçi üzerinden erişim (Rust 2024 `static_mut_refs`
/// uyarısını önlemek için; statiğe doğrudan referans almak yerine).
fn legacy() -> &'static mut UserSpace {
    unsafe { &mut *core::ptr::addr_of_mut!(LEGACY) }
}

fn flush_tlb() {
    unsafe {
        core::arch::asm!(
            "mov eax, cr3",
            "mov cr3, eax",
            out("eax") _,
            options(nostack, preserves_flags),
        );
    }
}

/// Yeni (boş) bir kullanıcı adres alanı kurmaya başlar: sayfa tablosunu ayırır
/// ve kullanıcı bölgesinin PDE'sini ona yönlendirir. TLB, `user_flush` ile
/// (tüm eşlemeler tamamlandıktan sonra) tek seferde temizlenir.
/// Tek süreç modu: yeni kullanıcı adres alanı kur (elf::load için).
pub fn user_begin() {
    unsafe {
        let pd = &*core::ptr::addr_of!(PAGE_DIR);
        let saved = &mut *core::ptr::addr_of_mut!(SAVED_PDE);
        for i in 0..USER_PDE_COUNT {
            saved[i] = pd.0[USER_PDE_IDX + i];
        }
    }
    *legacy() = UserSpace::new();
    legacy().activate();
}

/// Tek süreç modu: sayfa eşle.
pub fn user_map_page(vaddr: u32, writable: bool) -> usize {
    legacy().map_page(vaddr, writable)
}

/// Tek süreç modu: yığın kur.
pub fn user_setup_stack() -> u32 {
    legacy().setup_stack()
}

/// Aktif kullanıcı alanında aralık geçerli mi?
pub fn user_range_ok(ptr: u32, len: u32) -> bool {
    legacy().range_ok(ptr, len)
}

/// Çoklu süreç demosu öncesi mevcut kullanıcı PDE'lerini saklar.
pub fn save_user_pde_slot() {
    unsafe {
        let pd = &*core::ptr::addr_of!(PAGE_DIR);
        let saved = &mut *core::ptr::addr_of_mut!(SAVED_PDE);
        for i in 0..USER_PDE_COUNT {
            saved[i] = pd.0[USER_PDE_IDX + i];
        }
    }
}

/// Kimlik eşlemesine geri dön (çoklu süreç demosu sonrası).
pub fn user_restore_identity() {
    unsafe {
        let pd = &mut *core::ptr::addr_of_mut!(PAGE_DIR);
        let saved = &*core::ptr::addr_of!(SAVED_PDE);
        for i in 0..USER_PDE_COUNT {
            if saved[i] != 0 {
                pd.0[USER_PDE_IDX + i] = saved[i];
            } else {
                let base = ((USER_PDE_IDX + i) as u32).wrapping_mul(FOUR_MIB);
                pd.0[USER_PDE_IDX + i] = base | PRESENT | RW | PS;
            }
        }
        flush_tlb();
    }
}

/// `sbrk`: aktif (tek süreç) kullanıcı alanının heap'ini büyütür/küçültür.
pub fn user_sbrk(inc: i32) -> u32 {
    legacy().sbrk(inc)
}

/// ELF yüklemesinden sonra heap başlangıcını ayarlar.
pub fn user_set_brk(addr: u32) {
    legacy().set_brk(addr);
}

/// Tüm kullanıcı eşlemeleri kurulduktan sonra PDE'leri (sayfa tablosu pencereleri)
/// `PAGE_DIR`'e yazar ve TLB'yi temizler. `map_page` tabloları tembel ayırdığından
/// PDE güncellemesi burada toplu yapılır.
pub fn user_flush() {
    legacy().activate();
}

/// Kullanıcı adres alanını yıkar: PDE'yi eski 4 MiB sayfasına döndürür, sayfa
/// tablosunu ve tüm kullanıcı çerçevelerini serbest bırakır.
pub fn user_end() {
    legacy().destroy();
    *legacy() = UserSpace::EMPTY;
    user_restore_identity();
}

// --- Ayırma yapmayan (heap'siz) günlükleme yardımcıları ---

fn log_num(label: &str, v: usize, unit: &str) {
    serial::write_str(label);
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    let mut n = v;
    if n == 0 {
        i -= 1;
        buf[i] = b'0';
    }
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if let Ok(s) = core::str::from_utf8(&buf[i..]) {
        serial::write_str(s);
    }
    serial::write_str(unit);
}

fn log_hex(label: &str, v: usize) {
    serial::write_str(label);
    serial::write_str("0x");
    let mut buf = [0u8; 8];
    for j in 0..8 {
        let nib = ((v >> ((7 - j) * 4)) & 0xF) as u8;
        buf[j] = if nib < 10 {
            b'0' + nib
        } else {
            b'a' + (nib - 10)
        };
    }
    if let Ok(s) = core::str::from_utf8(&buf) {
        serial::write_str(s);
    }
}
