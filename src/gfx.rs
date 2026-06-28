//! Doğrusal framebuffer (grafik modu) sürücüsü.
//!
//! `boot.asm` Multiboot başlığında grafik modu istediği için GRUB/QEMU bizi
//! piksel piksel çizebileceğimiz doğrusal bir framebuffer ile başlatır ve
//! ayrıntıları Multiboot bilgi yapısında bırakır. Burada o yapıyı çözer ve
//! temel çizim ilkellerini (piksel, dikdörtgen, çizgi) sağlarız.
//!
//! Renkler dışarıya `0x00RRGGBB` biçiminde verilir; framebuffer'ın gerçek
//! bit düzenine göre `pack()` ile donanım biçimine çevrilir.

// Multiboot bilgi yapısındaki framebuffer alanlarının ofsetleri.
const FLAGS_OFF: usize = 0;
const FB_ADDR_OFF: usize = 88; // u64
const FB_PITCH_OFF: usize = 96; // u32
const FB_WIDTH_OFF: usize = 100; // u32
const FB_HEIGHT_OFF: usize = 104; // u32
const FB_BPP_OFF: usize = 108; // u8
const FB_TYPE_OFF: usize = 109; // u8

const FLAG_FRAMEBUFFER: u32 = 1 << 12;
const FB_TYPE_RGB: u8 = 1;

struct FrameBuffer {
    addr: usize,
    pitch: usize,
    width: usize,
    height: usize,
    bytes_pp: usize,
    r_pos: u32,
    g_pos: u32,
    b_pos: u32,
}

static mut FB: Option<FrameBuffer> = None;

#[allow(static_mut_refs)]
fn fb() -> Option<&'static FrameBuffer> {
    unsafe { FB.as_ref() }
}

unsafe fn rd_u32(base: u32, off: usize) -> u32 {
    core::ptr::read_unaligned((base as usize + off) as *const u32)
}

unsafe fn rd_u8(base: u32, off: usize) -> u8 {
    core::ptr::read_unaligned((base as usize + off) as *const u8)
}

/// Multiboot bilgi yapısını çözer. Geçerli bir RGB framebuffer varsa onu
/// kaydeder ve `true` döndürür; aksi halde grafik modu devre dışı kalır.
pub fn init(mbi: u32) -> bool {
    if mbi == 0 {
        return false;
    }
    unsafe {
        let flags = rd_u32(mbi, FLAGS_OFF);
        if flags & FLAG_FRAMEBUFFER == 0 {
            return false;
        }
        if rd_u8(mbi, FB_TYPE_OFF) != FB_TYPE_RGB {
            return false; // metin tipi framebuffer'ı desteklemiyoruz
        }
        let addr_lo = rd_u32(mbi, FB_ADDR_OFF);
        let addr_hi = rd_u32(mbi, FB_ADDR_OFF + 4);
        if addr_hi != 0 || addr_lo == 0 {
            return false; // 4 GiB üstü adresleri (sayfalama yok) kullanamayız
        }
        let bpp = rd_u8(mbi, FB_BPP_OFF) as usize;
        if bpp < 15 {
            return false;
        }
        // GRUB'ın bildirdiği renk alanı konumları bazı sürümlerde (QEMU std
        // VGA dahil) gerçek bellek düzeniyle tutarsız oluyor. PC framebuffer'ları
        // pratikte standart düzendedir, bu yüzden bpp'ye göre sabitliyoruz:
        //   32/24 bit -> 0x00RRGGBB (R@16, G@8, B@0)
        //   16 bit    -> 5:6:5      (R@11, G@5, B@0)
        let (r_pos, g_pos, b_pos) = if bpp >= 24 { (16, 8, 0) } else { (11, 5, 0) };
        FB = Some(FrameBuffer {
            addr: addr_lo as usize,
            pitch: rd_u32(mbi, FB_PITCH_OFF) as usize,
            width: rd_u32(mbi, FB_WIDTH_OFF) as usize,
            height: rd_u32(mbi, FB_HEIGHT_OFF) as usize,
            bytes_pp: bpp / 8,
            r_pos,
            g_pos,
            b_pos,
        });
    }
    true
}

/// Grafik modu kullanılabilir mi?
pub fn is_active() -> bool {
    fb().is_some()
}

pub fn width() -> usize {
    fb().map(|f| f.width).unwrap_or(0)
}

pub fn height() -> usize {
    fb().map(|f| f.height).unwrap_or(0)
}

/// (r, g, b) bileşenlerini framebuffer'ın donanım piksel biçimine paketler.
pub fn pack(r: u8, g: u8, b: u8) -> u32 {
    match fb() {
        Some(f) => ((r as u32) << f.r_pos) | ((g as u32) << f.g_pos) | ((b as u32) << f.b_pos),
        None => 0,
    }
}

/// 0x00RRGGBB biçimindeki bir rengi paketler.
pub fn rgb(hex: u32) -> u32 {
    pack((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

#[inline]
fn write_pixel(f: &FrameBuffer, x: usize, y: usize, packed: u32) {
    let off = f.addr + y * f.pitch + x * f.bytes_pp;
    unsafe {
        match f.bytes_pp {
            4 => core::ptr::write_volatile(off as *mut u32, packed),
            3 => {
                let p = off as *mut u8;
                core::ptr::write_volatile(p, packed as u8);
                core::ptr::write_volatile(p.add(1), (packed >> 8) as u8);
                core::ptr::write_volatile(p.add(2), (packed >> 16) as u8);
            }
            _ => core::ptr::write_volatile(off as *mut u16, packed as u16),
        }
    }
}

/// Tek bir pikselin paketlenmiş değerini okur (imleç altını saklamak için).
pub fn get_pixel(x: usize, y: usize) -> u32 {
    if let Some(f) = fb() {
        if x < f.width && y < f.height {
            let off = f.addr + y * f.pitch + x * f.bytes_pp;
            unsafe {
                return match f.bytes_pp {
                    4 => core::ptr::read_volatile(off as *const u32),
                    3 => {
                        let p = off as *const u8;
                        (core::ptr::read_volatile(p) as u32)
                            | ((core::ptr::read_volatile(p.add(1)) as u32) << 8)
                            | ((core::ptr::read_volatile(p.add(2)) as u32) << 16)
                    }
                    _ => core::ptr::read_volatile(off as *const u16) as u32,
                };
            }
        }
    }
    0
}

/// Paketlenmiş bir değeri doğrudan (paketlemeden) piksele yazar.
pub fn put_raw(x: usize, y: usize, packed: u32) {
    if let Some(f) = fb() {
        if x < f.width && y < f.height {
            write_pixel(f, x, y, packed);
        }
    }
}

/// Tek bir piksel çizer (paketlenmiş renkle). Ekran dışı koordinatlar yok sayılır.
#[allow(dead_code)]
pub fn put_pixel(x: usize, y: usize, packed: u32) {
    if let Some(f) = fb() {
        if x < f.width && y < f.height {
            write_pixel(f, x, y, packed);
        }
    }
}

/// Dolu bir dikdörtgen çizer (paketlenmiş renkle).
pub fn fill_rect(x: usize, y: usize, w: usize, h: usize, packed: u32) {
    if let Some(f) = fb() {
        let x1 = core::cmp::min(x + w, f.width);
        let y1 = core::cmp::min(y + h, f.height);
        let mut yy = y;
        while yy < y1 {
            let mut xx = x;
            while xx < x1 {
                write_pixel(f, xx, yy, packed);
                xx += 1;
            }
            yy += 1;
        }
    }
}

/// Dikdörtgen çerçeve (içi boş) çizer.
pub fn rect(x: usize, y: usize, w: usize, h: usize, packed: u32) {
    if w == 0 || h == 0 {
        return;
    }
    fill_rect(x, y, w, 1, packed);
    fill_rect(x, y + h - 1, w, 1, packed);
    fill_rect(x, y, 1, h, packed);
    fill_rect(x + w - 1, y, 1, h, packed);
}

/// Tüm ekranı tek renge boyar.
pub fn clear(packed: u32) {
    if let Some(f) = fb() {
        fill_rect(0, 0, f.width, f.height, packed);
    }
}

/// Kullanıcı alanından gelen 0x00RRGGBB piksel tamponunu (`w`×`h`) ekrana
/// `(dx, dy)` konumuna kopyalar; her pikseli donanım biçimine paketler.
/// `SYS_BLIT` sistem çağrısı bunu kullanır (kullanıcı kendi tamponunda çizer,
/// tek çağrıyla ekrana basar). Ekran dışına taşan kısımlar kırpılır.
pub fn blit_rgb(dx: usize, dy: usize, w: usize, h: usize, src: *const u32) {
    if let Some(f) = fb() {
        for j in 0..h {
            let py = dy + j;
            if py >= f.height {
                break;
            }
            for i in 0..w {
                let px = dx + i;
                if px >= f.width {
                    continue;
                }
                let hexv = unsafe { core::ptr::read_unaligned(src.add(j * w + i)) };
                let packed = pack((hexv >> 16) as u8, (hexv >> 8) as u8, hexv as u8);
                write_pixel(f, px, py, packed);
            }
        }
    }
}

// --- Bochs/QEMU VBE: çalışma anında grafik mod (BIOS'suz) ---
//
// QEMU'nun standart VGA aygıtı, Bochs VBE "DISPI" yazmaçlarını destekler. Bu
// yazmaçlarla (BIOS çağrısı olmadan) bir grafik modu seçip doğrusal framebuffer
// elde edebiliriz. Böylece QEMU'nun -kernel modunda (framebuffer vermese de)
// metin modundan grafik moduna geçebiliriz. VBE'yi kapatınca metin moduna döner.

use crate::port::{inw, outw};

const VBE_INDEX: u16 = 0x01CE;
const VBE_DATA: u16 = 0x01CF;
const VBE_XRES: u16 = 1;
const VBE_YRES: u16 = 2;
const VBE_BPP: u16 = 3;
const VBE_ENABLE: u16 = 4;
const VBE_VIRT_WIDTH: u16 = 6;
const VBE_ENABLED: u16 = 0x01;
const VBE_LFB: u16 = 0x40;

unsafe fn vbe_write(index: u16, val: u16) {
    outw(VBE_INDEX, index);
    outw(VBE_DATA, val);
}

unsafe fn vbe_read(index: u16) -> u16 {
    outw(VBE_INDEX, index);
    inw(VBE_DATA)
}

fn set_fb(addr: usize, pitch: usize, width: usize, height: usize, bpp: usize) {
    let (r_pos, g_pos, b_pos) = if bpp >= 24 { (16, 8, 0) } else { (11, 5, 0) };
    unsafe {
        FB = Some(FrameBuffer {
            addr,
            pitch,
            width,
            height,
            bytes_pp: bpp / 8,
            r_pos,
            g_pos,
            b_pos,
        });
    }
}

/// Bochs VBE ile bir grafik modu açar ve framebuffer'ı kurar. Başarılı olursa
/// `true` döner ve `is_active()` artık `true`dur. (QEMU / VirtualBox)
pub fn enable_bochs(width: usize, height: usize, bpp: usize) -> bool {
    let lfb = match crate::pci::find_lfb() {
        Some(a) => a as usize,
        None => return false,
    };
    unsafe {
        vbe_write(VBE_ENABLE, 0); // önce kapat
        vbe_write(VBE_XRES, width as u16);
        vbe_write(VBE_YRES, height as u16);
        vbe_write(VBE_VIRT_WIDTH, width as u16);
        vbe_write(VBE_BPP, bpp as u16);
        vbe_write(VBE_ENABLE, VBE_ENABLED | VBE_LFB);

        if vbe_read(VBE_ENABLE) & VBE_ENABLED == 0 {
            return false; // aygıt VBE'yi desteklemiyor
        }
    }
    set_fb(lfb, width * (bpp / 8), width, height, bpp);
    true
}
