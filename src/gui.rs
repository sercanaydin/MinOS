//! Basit grafik masaüstü ve pencere yöneticisi.
//!
//! Dosya sistemindeki dosyaları masaüstünde ikon olarak gösterir. Fare ile
//! bir ikona tıklanınca içeriğini bir pencerede açar. F1 veya ESC ile
//! terminale (metin görünümüne) dönülür.
//!
//! Her şey framebuffer'a doğrudan çizilir; bellek ayırıcı yoktur, bu yüzden
//! sabit boyutlu tamponlar kullanırız.

use crate::font_data;
use crate::fs;
use crate::gfx;
use crate::keyboard::{self, KEY_ESC, KEY_TOGGLE};
use crate::{input, mouse};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// --- Renkler (0x00RRGGBB) ---
const C_DESKTOP: u32 = 0x1B2838;
const C_TOPBAR: u32 = 0x2F5D8C;
const C_TOPBAR_TXT: u32 = 0xFFFFFF;
const C_TASKBAR: u32 = 0x141B24;
const C_TASKBAR_TXT: u32 = 0x9FB3C8;
const C_ICON_CARD: u32 = 0xECF0F1;
const C_ICON_TOP: u32 = 0x4AA3DF;
const C_ICON_LINE: u32 = 0xBDC3C7;
const C_LABEL: u32 = 0xECF0F1;
const C_WIN_TITLE: u32 = 0x2F5D8C;
const C_WIN_BODY: u32 = 0x0E1620;
const C_WIN_TXT: u32 = 0xD7E2EC;
const C_WIN_BORDER: u32 = 0x4AA3DF;
const C_CLOSE: u32 = 0xE74C3C;

const TOPBAR_H: usize = 34;
const TASKBAR_H: usize = 28;

const CELL_W: usize = 110;
const CELL_H: usize = 98;
const ICON_W: usize = 56;
const ICON_H: usize = 46;

// --- İkon tablosu (tıklama için) ---
// action: 0 = dosya (aç), 1 = dizin (içine gir), 2 = üst dizine çık
#[derive(Clone, Copy)]
struct Icon {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    file: usize, // fs dizin indeksi
    action: u8,
}

static mut ICONS: [Icon; fs::MAX_FILES] = [Icon {
    x: 0,
    y: 0,
    w: 0,
    h: 0,
    file: 0,
    action: 0,
}; fs::MAX_FILES];
static mut ICON_COUNT: usize = 0;

// GUI'nin gösterdiği geçerli dizin (kök = fs::ROOT).
static mut GUI_DIR: u8 = fs::ROOT;

// Açık pencere durumu.
static mut WIN_OPEN: bool = false;
static mut CLOSE_BTN: (usize, usize, usize, usize) = (0, 0, 0, 0);

// Dosya okuma tamponu.
static mut GUI_BUF: [u8; fs::MAX_FILE_SIZE] = [0; fs::MAX_FILE_SIZE];

// --- Metin editörü durumu ---
static mut EDITOR_OPEN: bool = false;
static mut ED_BUF: [u8; fs::MAX_FILE_SIZE] = [0; fs::MAX_FILE_SIZE];
static mut ED_LEN: usize = 0;
// İmleç konumu (ED_BUF içinde bayt ofseti). Ok tuşlarıyla gezinir; ekleme/silme
// bu noktada olur.
static mut ED_CUR: usize = 0;
static mut ED_NAME: [u8; 28] = [0; 28];
static mut ED_NLEN: usize = 0;
static mut ED_DIR: u8 = fs::ROOT;
static mut ED_WIN: (usize, usize, usize, usize) = (0, 0, 0, 0);
static mut ED_CLOSE: (usize, usize, usize, usize) = (0, 0, 0, 0);
static mut ED_SAVE: (usize, usize, usize, usize) = (0, 0, 0, 0);
static mut ED_DIRTY: bool = false;
// Üst bardaki "[+ Kod]" (yeni dosya) düğmesi.
static mut EDIT_BTN: (usize, usize, usize, usize) = (0, 0, 0, 0);

// --- Tarayıcı durumu ---
const C_LINK: u32 = 0x5DADE2;
const C_ADDR_BG: u32 = 0x06243A;
const C_BTN: u32 = 0x35577A;

/// Ayrıştırılmış sayfa: görünür metin + bağlantı aralıkları + çözülmüş URL'ler.
struct Page {
    text: String,
    links: Vec<String>,
    spans: Vec<(u32, u32, u32)>, // (text içinde başlangıç bayt, bitiş bayt, link indeksi)
    title: String,
}

static mut BROWSER_OPEN: bool = false;
static mut BROWSER_BTN: (usize, usize, usize, usize) = (0, 0, 0, 0);

// Adres çubuğu (yığın yok; sabit tampon).
const URL_MAX: usize = 256;
static mut URL_BUF: [u8; URL_MAX] = [0; URL_MAX];
static mut URL_LEN: usize = 0;

static mut PAGE: Option<Page> = None;
static mut HIST: Vec<String> = Vec::new();

// Geçerli konum (göreli bağlantı çözümü için).
static mut CUR_HTTPS: bool = false;
static mut CUR_HOST: String = String::new();
static mut CUR_PATH: String = String::new();

// İçerik alanı geometrisi ve kaydırma.
static mut SCROLL: usize = 0;
static mut TOTAL_LINES: usize = 0;
static mut CONT_X: usize = 0;
static mut CONT_Y: usize = 0;
static mut CONT_COLS: usize = 0;
static mut CONT_ROWS: usize = 0;
const LINE_H: usize = 10;

// Pencere ve adres çubuğu dikdörtgenleri.
static mut BWIN: (usize, usize, usize, usize) = (0, 0, 0, 0);
static mut ADDR_RECT: (usize, usize, usize, usize) = (0, 0, 0, 0);

// Tarayıcı düğme bölgeleri.
static mut BR_CLOSE: (usize, usize, usize, usize) = (0, 0, 0, 0);
static mut BR_BACK: (usize, usize, usize, usize) = (0, 0, 0, 0);
static mut BR_GO: (usize, usize, usize, usize) = (0, 0, 0, 0);
static mut BR_UP: (usize, usize, usize, usize) = (0, 0, 0, 0);
static mut BR_DOWN: (usize, usize, usize, usize) = (0, 0, 0, 0);

// Ekrandaki tıklanabilir bağlantı dikdörtgenleri (her çizimde yenilenir).
const LINK_RECTS_MAX: usize = 160;
static mut LINK_RECTS: [(usize, usize, usize, usize, usize); LINK_RECTS_MAX] =
    [(0, 0, 0, 0, 0); LINK_RECTS_MAX];
static mut LINK_RECT_COUNT: usize = 0;

// İmleç altını saklamak için tampon.
const CUR_W: usize = 12;
const CUR_H: usize = 16;
static mut CUR_SAVE: [u32; CUR_W * CUR_H] = [0; CUR_W * CUR_H];
static mut CUR_X: usize = 0;
static mut CUR_Y: usize = 0;
static mut CUR_VALID: bool = false;

// İmleç deseni: ' ' saydam, 'B' siyah kenar, 'W' beyaz dolgu.
static CURSOR: [&str; CUR_H] = [
    "B           ",
    "BB          ",
    "BWB         ",
    "BWWB        ",
    "BWWWB       ",
    "BWWWWB      ",
    "BWWWWWB     ",
    "BWWWWWWB    ",
    "BWWWWWWWB   ",
    "BWWWWWBBBB  ",
    "BWWBWWB     ",
    "BWB BWWB    ",
    "BB  BWWB    ",
    "B    BWWB   ",
    "      BWWB  ",
    "       BB   ",
];

// --- Metin çizimi (8x8 font, ölçeklenebilir) ---

fn draw_glyph(px: usize, py: usize, c: char, fg: u32, bg: Option<u32>, scale: usize) {
    let g = font_data::glyph(c);
    for (ry, bits) in g.iter().enumerate() {
        for cx in 0..8 {
            let on = (bits >> cx) & 1 != 0;
            if on {
                gfx::fill_rect(px + cx * scale, py + ry * scale, scale, scale, fg);
            } else if let Some(b) = bg {
                gfx::fill_rect(px + cx * scale, py + ry * scale, scale, scale, b);
            }
        }
    }
}

fn text(x: usize, y: usize, s: &str, fg: u32, bg: Option<u32>, scale: usize) {
    let mut cx = x;
    for ch in s.chars() {
        draw_glyph(cx, y, ch, fg, bg, scale);
        cx += 8 * scale;
    }
}

/// Dosya adını (UTF-8) bir tampona kopyalar; ekranda gösterim için.
fn name_str(name: &[u8], out: &mut [u8]) -> usize {
    let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    let n = core::cmp::min(len, out.len());
    out[..n].copy_from_slice(&name[..n]);
    n
}

// --- Masaüstü çizimi ---

fn draw_topbar() {
    let w = gfx::width();
    gfx::fill_rect(0, 0, w, TOPBAR_H, gfx::rgb(C_TOPBAR));
    text(12, 9, "MinOS", gfx::rgb(C_TOPBAR_TXT), None, 2);

    // Tıklanabilir "Internet" düğmesi.
    let bx = 108usize;
    let bw = 9 * 16 + 12; // " Internet " ~ 9 karakter, ölçek 2
    let bh = 24usize;
    let by = 5usize;
    gfx::fill_rect(bx, by, bw, bh, gfx::rgb(C_BTN));
    gfx::rect(bx, by, bw, bh, gfx::rgb(0x6FA8D0));
    text(bx + 8, by + 5, "Internet", gfx::rgb(0xFFFFFF), None, 2);
    unsafe { BROWSER_BTN = (bx, by, bw, bh) };

    // Tıklanabilir "[+ Kod]" (yeni metin dosyası / kod editörü) düğmesi.
    let ex = bx + bw + 10;
    let ew = 6 * 16 + 12; // "+ Kod " ~ 6 karakter
    gfx::fill_rect(ex, by, ew, bh, gfx::rgb(0x2E7D5B));
    gfx::rect(ex, by, ew, bh, gfx::rgb(0x57C99A));
    text(ex + 8, by + 5, "+ Kod", gfx::rgb(0xFFFFFF), None, 2);
    unsafe { EDIT_BTN = (ex, by, ew, bh) };

    let hint = "[F1]/[ESC] terminal";
    let hx = w.saturating_sub(hint.chars().count() * 16 + 12);
    text(hx, 9, hint, gfx::rgb(0xCfe0f0), None, 2);
    draw_clock();
}

/// Üst bardaki saat/tarih alanının (x, genişlik) değerleri.
/// Ortalanır; ancak "[+ Kod]" düğmesinin sağında kalır ki saniyelik yenileme
/// düğmenin üzerine yazıp silmesin.
fn clock_region() -> (usize, usize) {
    let chars = 19; // "GG.AA.YYYY SS:DD:ss"
    let cw = chars * 16; // ölçek 2 → karakter 16px
    let centered = gfx::width().saturating_sub(cw) / 2;
    let min_x = 392; // [+ Kod] düğmesinin sağ kenarı + boşluk
    let x = if centered < min_x { min_x } else { centered };
    (x, cw)
}

/// Üst barda tarih + saati çizer (her saniye yenilenebilir).
fn draw_clock() {
    let dt = crate::rtc::now();
    let mut b = [0u8; 24];
    let mut n = 0;
    n += fmt2(&mut b[n..], dt.day as u32);
    b[n] = b'.';
    n += 1;
    n += fmt2(&mut b[n..], dt.month as u32);
    b[n] = b'.';
    n += 1;
    n += put_num(&mut b[n..], dt.year as u32);
    b[n] = b' ';
    n += 1;
    n += fmt2(&mut b[n..], dt.hour as u32);
    b[n] = b':';
    n += 1;
    n += fmt2(&mut b[n..], dt.min as u32);
    b[n] = b':';
    n += 1;
    n += fmt2(&mut b[n..], dt.sec as u32);

    let (x, cw) = clock_region();
    gfx::fill_rect(x, 0, cw, TOPBAR_H, gfx::rgb(C_TOPBAR));
    if let Ok(s) = core::str::from_utf8(&b[..n]) {
        text(x, 9, s, gfx::rgb(C_TOPBAR_TXT), None, 2);
    }
}

/// İki haneli, sıfır dolgulu sayı yazar (ör. 07).
fn fmt2(dst: &mut [u8], v: u32) -> usize {
    if dst.len() < 2 {
        return 0;
    }
    dst[0] = b'0' + ((v / 10) % 10) as u8;
    dst[1] = b'0' + (v % 10) as u8;
    2
}

fn draw_taskbar(count: usize) {
    let h = gfx::height();
    let w = gfx::width();
    let y = h - TASKBAR_H;
    gfx::fill_rect(0, y, w, TASKBAR_H, gfx::rgb(C_TASKBAR));

    // Konum (yol) + öğe sayısı + disk kullanımı.
    let mut buf = [0u8; 96];
    let mut n = 0;
    n += put_str(&mut buf[n..], "Konum: ");
    n += append_path(&mut buf[n..], unsafe { GUI_DIR });
    n += put_str(&mut buf[n..], "   Oge: ");
    n += put_num(&mut buf[n..], count as u32);
    if let Ok((used, total)) = fs::usage() {
        n += put_str(&mut buf[n..], "   Disk: ");
        n += put_num(&mut buf[n..], used * 512 / 1024);
        n += put_str(&mut buf[n..], "/");
        n += put_num(&mut buf[n..], total * 512 / 1024);
        n += put_str(&mut buf[n..], " KiB");
    }
    if let Ok(s) = core::str::from_utf8(&buf[..n]) {
        text(12, y + 6, s, gfx::rgb(C_TASKBAR_TXT), None, 2);
    }
}

/// Geçerli dizin yolunu '/' ile başlayarak `dst` içine yazar.
fn append_path(dst: &mut [u8], dir: u8) -> usize {
    if dir == fs::ROOT {
        return put_str(dst, "/");
    }
    let mut chain = [0u8; fs::MAX_FILES];
    let mut nn = 0;
    let mut cur = dir;
    while cur != fs::ROOT && nn < chain.len() {
        chain[nn] = cur;
        nn += 1;
        cur = fs::parent_of(cur).unwrap_or(fs::ROOT);
    }
    let mut w = 0;
    for i in (0..nn).rev() {
        w += put_str(&mut dst[w..], "/");
        if let Ok(Some(info)) = fs::entry_info(chain[i] as usize) {
            let mut nb = [0u8; 28];
            let n = name_str(&info.name, &mut nb);
            if let Ok(s) = core::str::from_utf8(&nb[..n]) {
                w += put_str(&mut dst[w..], s);
            }
        }
    }
    w
}

/// Tek bir dosya ikonu (belge görünümü) ve adını çizer.
fn draw_icon(cell_x: usize, cell_y: usize, name: &[u8], size: u32) {
    let ix = cell_x + (CELL_W - ICON_W) / 2;
    let iy = cell_y;

    // Belge kartı.
    gfx::fill_rect(ix, iy, ICON_W, ICON_H, gfx::rgb(C_ICON_CARD));
    gfx::fill_rect(ix, iy, ICON_W, 12, gfx::rgb(C_ICON_TOP));
    gfx::rect(ix, iy, ICON_W, ICON_H, gfx::rgb(C_ICON_LINE));
    // İçeride metin satırı izlenimi.
    for k in 0..3 {
        gfx::fill_rect(ix + 8, iy + 20 + k * 8, ICON_W - 16, 2, gfx::rgb(C_ICON_LINE));
    }

    // Ad (en çok ~12 karakter göster).
    let mut nbuf = [0u8; 28];
    let nn = name_str(name, &mut nbuf);
    let max_chars = 12;
    let show = core::cmp::min(nn, max_chars);
    if let Ok(s) = core::str::from_utf8(&nbuf[..show]) {
        let tx = cell_x + 6;
        text(tx, iy + ICON_H + 6, s, gfx::rgb(C_LABEL), None, 1);
    }
    // Boyut.
    let mut sb = [0u8; 16];
    let mut sn = 0;
    sn += put_num(&mut sb[sn..], size);
    sn += put_str(&mut sb[sn..], " B");
    if let Ok(s) = core::str::from_utf8(&sb[..sn]) {
        text(cell_x + 6, iy + ICON_H + 18, s, gfx::rgb(0x7F8C9A), None, 1);
    }
}

/// Bir klasör ikonu ve adını çizer.
fn draw_folder(cell_x: usize, cell_y: usize, name: &[u8]) {
    let ix = cell_x + (CELL_W - ICON_W) / 2;
    let iy = cell_y;
    let body = gfx::rgb(0xF1C40F);
    let edge = gfx::rgb(0xB7950B);

    // Üst sekme + gövde (klasör görünümü).
    gfx::fill_rect(ix, iy + 4, ICON_W / 2, 8, body);
    gfx::fill_rect(ix, iy + 10, ICON_W, ICON_H - 10, body);
    gfx::rect(ix, iy + 10, ICON_W, ICON_H - 10, edge);

    // Ad.
    let mut nbuf = [0u8; 28];
    let nn = name_str(name, &mut nbuf);
    let show = core::cmp::min(nn, 12);
    if let Ok(s) = core::str::from_utf8(&nbuf[..show]) {
        text(cell_x + 6, iy + ICON_H + 6, s, gfx::rgb(C_LABEL), None, 1);
    }
    text(cell_x + 6, iy + ICON_H + 18, "<dizin>", gfx::rgb(0xF1C40F), None, 1);
}

/// "Üst dizine çık" ikonu (geri ok içeren klasör).
fn draw_up_icon(cell_x: usize, cell_y: usize) {
    let ix = cell_x + (CELL_W - ICON_W) / 2;
    let iy = cell_y;
    let body = gfx::rgb(0x95A5A6);
    let edge = gfx::rgb(0x6C7A7B);
    gfx::fill_rect(ix, iy + 4, ICON_W / 2, 8, body);
    gfx::fill_rect(ix, iy + 10, ICON_W, ICON_H - 10, body);
    gfx::rect(ix, iy + 10, ICON_W, ICON_H - 10, edge);
    // Büyük ".." işareti.
    text(ix + ICON_W / 2 - 8, iy + 18, "..", gfx::rgb(0x1B2838), None, 2);
    text(cell_x + 6, iy + ICON_H + 6, "ust dizin", gfx::rgb(C_LABEL), None, 1);
}

/// Tüm masaüstünü (arka plan, başlık, ikonlar, görev çubuğu) çizer.
pub fn draw_desktop() {
    gfx::clear(gfx::rgb(C_DESKTOP));
    draw_topbar();

    let start_x = 20;
    let start_y = TOPBAR_H + 18;
    let area_w = gfx::width();
    let cols = core::cmp::max(1, (area_w - start_x) / CELL_W);

    let here = unsafe { GUI_DIR };
    let mut count = 0usize;
    let mut slot = 0usize;

    // Kök değilsek ilk hücre "üst dizine çık".
    if here != fs::ROOT {
        let cx = start_x + (slot % cols) * CELL_W;
        let cy = start_y + (slot / cols) * CELL_H;
        draw_up_icon(cx, cy);
        unsafe {
            ICONS[count] = Icon {
                x: cx,
                y: cy,
                w: CELL_W - 8,
                h: CELL_H - 8,
                file: 0,
                action: 2,
            };
        }
        count += 1;
        slot += 1;
    }

    let mut entries = 0usize;
    for i in 0..fs::MAX_FILES {
        if let Ok(Some(info)) = fs::entry_info(i) {
            // Yalnızca geçerli dizinin içindekiler.
            if info.parent != here {
                continue;
            }
            let cx = start_x + (slot % cols) * CELL_W;
            let cy = start_y + (slot / cols) * CELL_H;
            let action = if info.is_dir() {
                draw_folder(cx, cy, &info.name);
                1
            } else {
                draw_icon(cx, cy, &info.name, info.size);
                0
            };
            unsafe {
                ICONS[count] = Icon {
                    x: cx,
                    y: cy,
                    w: CELL_W - 8,
                    h: CELL_H - 8,
                    file: i,
                    action,
                };
            }
            count += 1;
            slot += 1;
            entries += 1;
        }
    }
    unsafe { ICON_COUNT = count };

    if entries == 0 {
        let ty = start_y + if here != fs::ROOT { CELL_H } else { 0 } + 4;
        text(
            start_x,
            ty,
            "(bos dizin)",
            gfx::rgb(0x7F8C9A),
            None,
            2,
        );
    }

    draw_taskbar(entries);
    unsafe { WIN_OPEN = false };
}

// --- Pencere (dosya görüntüleyici) ---

fn open_file(file_index: usize) {
    let info = match fs::entry_info(file_index) {
        Ok(Some(i)) => i,
        _ => return,
    };

    let w = gfx::width();
    let h = gfx::height();
    let win_w = core::cmp::min(680, w - 60);
    let win_h = core::cmp::min(460, h - 80);
    let wx = (w - win_w) / 2;
    let wy = (h - win_h) / 2;
    let title_h = 28usize;

    // Çerçeve ve gövde.
    gfx::fill_rect(wx, wy, win_w, title_h, gfx::rgb(C_WIN_TITLE));
    gfx::fill_rect(wx, wy + title_h, win_w, win_h - title_h, gfx::rgb(C_WIN_BODY));
    gfx::rect(wx, wy, win_w, win_h, gfx::rgb(C_WIN_BORDER));

    // Başlık (dosya adı).
    let mut nbuf = [0u8; 28];
    let nn = name_str(&info.name, &mut nbuf);
    if let Ok(s) = core::str::from_utf8(&nbuf[..nn]) {
        text(wx + 10, wy + 6, s, gfx::rgb(0xFFFFFF), None, 2);
    }

    // Kapat düğmesi [X].
    let btn = 20usize;
    let bx = wx + win_w - btn - 6;
    let by = wy + 4;
    gfx::fill_rect(bx, by, btn, btn, gfx::rgb(C_CLOSE));
    text(bx + 3, by + 3, "x", gfx::rgb(0xFFFFFF), None, 2);
    unsafe { CLOSE_BTN = (bx, by, btn, btn) };

    // İçerik.
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(GUI_BUF) };
    let read = fs::read_index(file_index, buf).unwrap_or(0);

    let pad = 10usize;
    let cont_x = wx + pad;
    let cont_y = wy + title_h + pad;
    let line_h = 10usize; // 8px glif + 2 boşluk
    let max_cols = (win_w - 2 * pad) / 8;
    let max_rows = (win_h - title_h - 2 * pad) / line_h;

    match core::str::from_utf8(&buf[..read]) {
        Ok(s) => draw_wrapped(cont_x, cont_y, s, max_cols, max_rows, line_h),
        Err(_) => {
            text(
                cont_x,
                cont_y,
                "(ikili veri — metin olarak gösterilemiyor)",
                gfx::rgb(0xE0A0A0),
                None,
                1,
            );
        }
    }

    unsafe { WIN_OPEN = true };
}

/// Metni pencere içine satır kaydırarak çizer.
fn draw_wrapped(x: usize, y: usize, s: &str, max_cols: usize, max_rows: usize, line_h: usize) {
    let mut col = 0usize;
    let mut row = 0usize;
    let fg = gfx::rgb(C_WIN_TXT);
    let bg = Some(gfx::rgb(C_WIN_BODY));
    for ch in s.chars() {
        if row >= max_rows {
            break;
        }
        if ch == '\n' {
            col = 0;
            row += 1;
            continue;
        }
        if ch == '\r' || ch == '\t' {
            continue;
        }
        if col >= max_cols {
            col = 0;
            row += 1;
            if row >= max_rows {
                break;
            }
        }
        draw_glyph(x + col * 8, y + row * line_h, ch, fg, bg, 1);
        col += 1;
    }
}

// ===========================================================================
// Metin / kod editörü
// ===========================================================================

/// "[+ Kod]" düğmesi: GUI dizininde boş yeni bir dosya (kod.c / kod2.c ...) açar.
fn open_editor_new() {
    let mut nm = [0u8; 28];
    let nl = pick_code_name(&mut nm);
    unsafe {
        let name = &mut *core::ptr::addr_of_mut!(ED_NAME);
        *name = [0u8; 28];
        name[..nl].copy_from_slice(&nm[..nl]);
        ED_NLEN = nl;
        ED_LEN = 0;
        ED_CUR = 0;
        ED_DIR = GUI_DIR;
        ED_DIRTY = false;
    }
    editor_open_common();
}

/// Var olan (metin) bir dosyayı düzenlemek için açar.
fn open_editor_file(file_index: usize) {
    let info = match fs::entry_info(file_index) {
        Ok(Some(i)) => i,
        _ => return,
    };
    let read = {
        let buf = unsafe { &mut *core::ptr::addr_of_mut!(ED_BUF) };
        fs::read_index(file_index, buf).unwrap_or(0)
    };
    let mut nm = [0u8; 28];
    let nl = name_str(&info.name, &mut nm);
    unsafe {
        let name = &mut *core::ptr::addr_of_mut!(ED_NAME);
        *name = [0u8; 28];
        name[..nl].copy_from_slice(&nm[..nl]);
        ED_NLEN = nl;
        ED_LEN = read;
        ED_CUR = read;
        ED_DIR = GUI_DIR;
        ED_DIRTY = false;
    }
    editor_open_common();
}

/// Tıklanan dosya metinse editörde, ikili ise salt-okunur görüntüleyicide açar.
fn open_file_or_editor(file_index: usize) {
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(GUI_BUF) };
    let read = fs::read_index(file_index, buf).unwrap_or(0);
    if core::str::from_utf8(&buf[..read]).is_ok() {
        open_editor_file(file_index);
    } else {
        open_file(file_index);
    }
}

/// GUI dizininde kullanımda olmayan ilk "kod*.c" adını üretir.
fn pick_code_name(out: &mut [u8; 28]) -> usize {
    let dir = unsafe { GUI_DIR };
    for n in 0..10u32 {
        let mut nm = [0u8; 28];
        let mut l = put_str(&mut nm, "kod");
        if n > 0 {
            nm[l] = b'0' + n as u8;
            l += 1;
        }
        l += put_str(&mut nm[l..], ".c");
        if !name_exists(&nm[..l], dir) {
            out[..l].copy_from_slice(&nm[..l]);
            return l;
        }
    }
    put_str(out, "kod.c")
}

fn name_exists(name: &[u8], parent: u8) -> bool {
    for i in 0..fs::MAX_FILES {
        if let Ok(Some(info)) = fs::entry_info(i) {
            if info.parent == parent {
                let mut nm = [0u8; 28];
                let nl = name_str(&info.name, &mut nm);
                if &nm[..nl] == name {
                    return true;
                }
            }
        }
    }
    false
}

fn editor_open_common() {
    unsafe {
        EDITOR_OPEN = true;
        WIN_OPEN = false;
        BROWSER_OPEN = false;
    }
    editor_draw_chrome();
    editor_render_body();
    present_cursor();
}

/// Editör penceresinin çerçevesini, başlığını ve düğmelerini çizer.
fn editor_draw_chrome() {
    let w = gfx::width();
    let h = gfx::height();
    let wx = 16usize;
    let wy = TOPBAR_H + 8;
    let ww = w - 32;
    let wh = h - TOPBAR_H - TASKBAR_H - 16;
    let title_h = 28usize;
    unsafe { ED_WIN = (wx, wy, ww, wh) };

    gfx::fill_rect(wx, wy, ww, title_h, gfx::rgb(C_WIN_TITLE));
    gfx::fill_rect(wx, wy + title_h, ww, wh - title_h, gfx::rgb(C_WIN_BODY));
    gfx::rect(wx, wy, ww, wh, gfx::rgb(C_WIN_BORDER));

    let mut title = String::from("Editor  ");
    unsafe {
        let nm = &*core::ptr::addr_of!(ED_NAME);
        if let Ok(s) = core::str::from_utf8(&nm[..ED_NLEN]) {
            title.push_str(s);
        }
    }
    text(wx + 10, wy + 6, &title, gfx::rgb(0xFFFFFF), None, 2);

    let btn = 20usize;
    let cx = wx + ww - btn - 6;
    let cy = wy + 4;
    gfx::fill_rect(cx, cy, btn, btn, gfx::rgb(C_CLOSE));
    text(cx + 3, cy + 3, "x", gfx::rgb(0xFFFFFF), None, 2);
    unsafe { ED_CLOSE = (cx, cy, btn, btn) };

    let sw = 6 * 16 + 12;
    let sx = cx - sw - 8;
    let sy = wy + 3;
    gfx::fill_rect(sx, sy, sw, btn + 1, gfx::rgb(0x2E7D5B));
    text(sx + 6, sy + 4, "Kaydet", gfx::rgb(0xFFFFFF), None, 2);
    unsafe { ED_SAVE = (sx, sy, sw, btn + 1) };
}

/// Editör gövdesini (metin + imleç + durum satırı) çizer.
fn editor_render_body() {
    let (wx, wy, ww, wh) = unsafe { ED_WIN };
    let title_h = 28usize;
    let status_h = 16usize;
    let pad = 10usize;
    let bx = wx + pad;
    let by = wy + title_h + pad;
    let line_h = LINE_H;
    let cols = (ww - 2 * pad) / 8;
    let rows = (wh - title_h - status_h - 2 * pad) / line_h;

    gfx::fill_rect(
        wx + 1,
        wy + title_h + 1,
        ww - 2,
        wh - title_h - status_h - 2,
        gfx::rgb(C_WIN_BODY),
    );

    let len = unsafe { ED_LEN };
    let cur = unsafe { core::cmp::min(ED_CUR, len) };
    let s = {
        let buf = unsafe { &*core::ptr::addr_of!(ED_BUF) };
        core::str::from_utf8(&buf[..len]).unwrap_or("")
    };
    let (cur_col, cur_row) = editor_layout(s, cols, cur);
    // İmleç satırını görünür tut.
    let scroll = if cur_row >= rows { cur_row - rows + 1 } else { 0 };

    let fg = gfx::rgb(C_WIN_TXT);
    let bg = Some(gfx::rgb(C_WIN_BODY));
    let mut col = 0usize;
    let mut row = 0usize;
    for ch in s.chars() {
        if ch == '\n' {
            col = 0;
            row += 1;
            continue;
        }
        if ch == '\r' {
            continue;
        }
        if col >= cols {
            col = 0;
            row += 1;
        }
        if row >= scroll && row < scroll + rows {
            draw_glyph(bx + col * 8, by + (row - scroll) * line_h, ch, fg, bg, 1);
        }
        col += 1;
    }
    if cur_row >= scroll && cur_row < scroll + rows {
        let caret_x = bx + cur_col * 8;
        let caret_y = by + (cur_row - scroll) * line_h;
        gfx::fill_rect(caret_x, caret_y, 8, line_h - 1, gfx::rgb(0x9AE6B4));
    }

    // Durum satırı.
    let sty = wy + wh - status_h;
    gfx::fill_rect(wx + 1, sty, ww - 2, status_h - 1, gfx::rgb(0x132235));
    let saved = unsafe { !ED_DIRTY };
    let msg = if saved {
        "kayitli  -  oklar=gez, [Kaydet] diske yaz, [X] kapat"
    } else {
        "* degisti  -  oklar=gez, [Kaydet] ile diske yaz"
    };
    let mc = if saved { 0x9AE6B4 } else { 0xF1C40F };
    text(wx + 8, sty + 4, msg, gfx::rgb(mc), None, 1);
}

/// Metni sarmalayarak `cur_off` bayt ofsetindeki imlecin (col, row) ekran
/// konumunu hesaplar. Sarma (wrap) `cols` sütununda olur.
fn editor_layout(s: &str, cols: usize, cur_off: usize) -> (usize, usize) {
    let mut col = 0usize;
    let mut row = 0usize;
    for (off, ch) in s.char_indices() {
        if off == cur_off {
            return (col, row);
        }
        if ch == '\n' {
            col = 0;
            row += 1;
            continue;
        }
        if ch == '\r' {
            continue;
        }
        if col >= cols {
            col = 0;
            row += 1;
        }
        col += 1;
    }
    (col, row) // imleç metnin sonunda
}

/// Verilen (want_row, want_col) ekran konumuna en yakın bayt ofsetini bulur.
/// Up/Down ok tuşları için kullanılır.
fn ed_off_at(s: &str, cols: usize, want_row: usize, want_col: usize) -> usize {
    let mut col = 0usize;
    let mut row = 0usize;
    for (off, ch) in s.char_indices() {
        if row == want_row && col >= want_col {
            return off;
        }
        if row > want_row {
            return off;
        }
        if ch == '\n' {
            if row == want_row {
                return off; // istenen satırın sonu
            }
            col = 0;
            row += 1;
            continue;
        }
        if ch == '\r' {
            continue;
        }
        if col >= cols {
            col = 0;
            row += 1;
            if row > want_row {
                return off;
            }
        }
        col += 1;
    }
    s.len()
}

/// İmleç konumuna (ED_CUR) bayt dizisi ekler.
fn ed_insert(b: &[u8]) {
    unsafe {
        let buf = &mut *core::ptr::addr_of_mut!(ED_BUF);
        if ED_LEN + b.len() > buf.len() {
            return;
        }
        let cur = core::cmp::min(ED_CUR, ED_LEN);
        buf.copy_within(cur..ED_LEN, cur + b.len());
        buf[cur..cur + b.len()].copy_from_slice(b);
        ED_LEN += b.len();
        ED_CUR = cur + b.len();
        ED_DIRTY = true;
    }
}

/// Sonraki UTF-8 karakter sınırının bayt uzunluğu (öndeki bayta göre).
fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else {
        4
    }
}

fn editor_key(c: char) {
    use crate::keyboard::{KEY_DEL, KEY_DOWN, KEY_END, KEY_HOME, KEY_LEFT, KEY_RIGHT, KEY_UP};
    let cols = {
        let (_, _, ww, _) = unsafe { ED_WIN };
        (ww - 20) / 8
    };
    match c {
        KEY_LEFT => unsafe {
            let buf = &*core::ptr::addr_of!(ED_BUF);
            if ED_CUR > 0 {
                ED_CUR -= 1;
                while ED_CUR > 0 && (buf[ED_CUR] & 0xC0) == 0x80 {
                    ED_CUR -= 1;
                }
            }
        },
        KEY_RIGHT => unsafe {
            let buf = &*core::ptr::addr_of!(ED_BUF);
            if ED_CUR < ED_LEN {
                ED_CUR += utf8_len(buf[ED_CUR]);
                if ED_CUR > ED_LEN {
                    ED_CUR = ED_LEN;
                }
            }
        },
        KEY_UP | KEY_DOWN => unsafe {
            let buf = &*core::ptr::addr_of!(ED_BUF);
            let s = core::str::from_utf8(&buf[..ED_LEN]).unwrap_or("");
            let (col, row) = editor_layout(s, cols, ED_CUR);
            let target = if c == KEY_UP {
                if row == 0 {
                    return;
                }
                row - 1
            } else {
                row + 1
            };
            ED_CUR = ed_off_at(s, cols, target, col);
        },
        KEY_HOME => unsafe {
            let buf = &*core::ptr::addr_of!(ED_BUF);
            let s = core::str::from_utf8(&buf[..ED_LEN]).unwrap_or("");
            let (_, row) = editor_layout(s, cols, ED_CUR);
            ED_CUR = ed_off_at(s, cols, row, 0);
        },
        KEY_END => unsafe {
            let buf = &*core::ptr::addr_of!(ED_BUF);
            let s = core::str::from_utf8(&buf[..ED_LEN]).unwrap_or("");
            let (_, row) = editor_layout(s, cols, ED_CUR);
            ED_CUR = ed_off_at(s, cols, row, usize::MAX);
        },
        KEY_DEL => unsafe {
            let buf = &mut *core::ptr::addr_of_mut!(ED_BUF);
            if ED_CUR < ED_LEN {
                let n = utf8_len(buf[ED_CUR]);
                let n = core::cmp::min(n, ED_LEN - ED_CUR);
                buf.copy_within(ED_CUR + n..ED_LEN, ED_CUR);
                ED_LEN -= n;
                ED_DIRTY = true;
            }
        },
        '\u{8}' => unsafe {
            // Backspace: imleçten önceki karakteri sil.
            let buf = &mut *core::ptr::addr_of_mut!(ED_BUF);
            if ED_CUR > 0 {
                let mut prev = ED_CUR - 1;
                while prev > 0 && (buf[prev] & 0xC0) == 0x80 {
                    prev -= 1;
                }
                let n = ED_CUR - prev;
                buf.copy_within(ED_CUR..ED_LEN, prev);
                ED_LEN -= n;
                ED_CUR = prev;
                ED_DIRTY = true;
            }
        },
        '\n' | '\r' => ed_insert(b"\n"),
        '\t' => ed_insert(b"  "),
        c if (c as u32) >= 0x20 && (c as u32) < 0xE000 && (c as u32) != 0x7F => {
            let mut tmp = [0u8; 4];
            let b = c.encode_utf8(&mut tmp);
            ed_insert(b.as_bytes());
        }
        _ => return,
    }
    editor_render_body();
    present_cursor();
}

fn editor_save() {
    let dir = unsafe { ED_DIR };
    let nl = unsafe { ED_NLEN };
    let len = unsafe { ED_LEN };
    let res = unsafe {
        let nm = &*core::ptr::addr_of!(ED_NAME);
        let buf = &*core::ptr::addr_of!(ED_BUF);
        fs::write_file(&nm[..nl], dir, &buf[..len])
    };
    if res.is_ok() {
        unsafe { ED_DIRTY = false };
    }
    editor_render_body();
    present_cursor();
}

fn close_editor() {
    unsafe { EDITOR_OPEN = false };
    draw_desktop();
    present_cursor();
}

fn editor_click(mx: usize, my: usize) {
    if inside(mx, my, unsafe { ED_CLOSE }) {
        close_editor();
    } else if inside(mx, my, unsafe { ED_SAVE }) {
        editor_save();
    }
}

// --- Fare imleci ---

fn save_under(x: usize, y: usize) {
    unsafe {
        for j in 0..CUR_H {
            for i in 0..CUR_W {
                CUR_SAVE[j * CUR_W + i] = gfx::get_pixel(x + i, y + j);
            }
        }
        CUR_X = x;
        CUR_Y = y;
        CUR_VALID = true;
    }
}

fn restore_under() {
    unsafe {
        if !CUR_VALID {
            return;
        }
        for j in 0..CUR_H {
            for i in 0..CUR_W {
                gfx::put_raw(CUR_X + i, CUR_Y + j, CUR_SAVE[j * CUR_W + i]);
            }
        }
    }
}

fn draw_cursor(x: usize, y: usize) {
    let black = gfx::rgb(0x000000);
    let white = gfx::rgb(0xFFFFFF);
    for (j, row) in CURSOR.iter().enumerate() {
        for (i, ch) in row.bytes().enumerate() {
            match ch {
                b'B' => gfx::put_raw(x + i, y + j, black),
                b'W' => gfx::put_raw(x + i, y + j, white),
                _ => {}
            }
        }
    }
}

/// İmleci yeni konumda yeniden çizer (eski yerini geri yükleyerek).
fn move_cursor(x: usize, y: usize) {
    restore_under();
    save_under(x, y);
    draw_cursor(x, y);
}

/// Tam yeniden çizimden sonra imleci yeniden konumlandırır.
fn present_cursor() {
    unsafe { CUR_VALID = false };
    let x = mouse::x();
    let y = mouse::y();
    save_under(x, y);
    draw_cursor(x, y);
}

fn inside(px: usize, py: usize, r: (usize, usize, usize, usize)) -> bool {
    px >= r.0 && px < r.0 + r.2 && py >= r.1 && py < r.1 + r.3
}

fn handle_click(mx: usize, my: usize) {
    if unsafe { BROWSER_OPEN } {
        browser_click(mx, my);
        return;
    }
    if unsafe { EDITOR_OPEN } {
        editor_click(mx, my);
        return;
    }
    unsafe {
        if WIN_OPEN {
            if inside(mx, my, CLOSE_BTN) {
                draw_desktop();
                present_cursor();
            }
            return;
        }
        if inside(mx, my, BROWSER_BTN) {
            open_browser();
            present_cursor();
            return;
        }
        if inside(mx, my, EDIT_BTN) {
            open_editor_new();
            return;
        }
        for k in 0..ICON_COUNT {
            let ic = ICONS[k];
            if inside(mx, my, (ic.x, ic.y, ic.w, ic.h)) {
                match ic.action {
                    2 => {
                        // Üst dizine çık.
                        GUI_DIR = fs::parent_of(GUI_DIR).unwrap_or(fs::ROOT);
                        draw_desktop();
                        present_cursor();
                    }
                    1 => {
                        // Dizine gir.
                        GUI_DIR = ic.file as u8;
                        draw_desktop();
                        present_cursor();
                    }
                    _ => {
                        open_file_or_editor(ic.file);
                        present_cursor();
                    }
                }
                return;
            }
        }
    }
}

// ===========================================================================
// Masaüstü tarayıcı
// ===========================================================================

fn open_browser() {
    unsafe {
        BROWSER_OPEN = true;
        WIN_OPEN = false;
        SCROLL = 0;
        if URL_LEN == 0 {
            let def = b"https://example.com";
            URL_BUF[..def.len()].copy_from_slice(def);
            URL_LEN = def.len();
        }
    }
    draw_browser_chrome();
    set_home_page();
    browser_render();
}

fn close_browser() {
    unsafe { BROWSER_OPEN = false };
    draw_desktop();
    present_cursor();
}

/// Pencere çerçevesini, başlığı, düğmeleri ve adres çubuğunu çizer.
fn draw_browser_chrome() {
    let w = gfx::width();
    let h = gfx::height();
    let wx = 16usize;
    let wy = TOPBAR_H + 8;
    let ww = w - 32;
    let wh = h - TOPBAR_H - TASKBAR_H - 16;
    let title_h = 28usize;
    let addr_h = 30usize;
    unsafe { BWIN = (wx, wy, ww, wh) };

    // Çerçeve.
    gfx::fill_rect(wx, wy, ww, title_h, gfx::rgb(C_WIN_TITLE));
    gfx::fill_rect(wx, wy + title_h, ww, wh - title_h, gfx::rgb(C_WIN_BODY));
    gfx::rect(wx, wy, ww, wh, gfx::rgb(C_WIN_BORDER));
    text(wx + 10, wy + 6, "Internet", gfx::rgb(0xFFFFFF), None, 2);

    // [X] kapat.
    let btn = 20usize;
    let cx = wx + ww - btn - 6;
    let cy = wy + 4;
    gfx::fill_rect(cx, cy, btn, btn, gfx::rgb(C_CLOSE));
    text(cx + 3, cy + 3, "x", gfx::rgb(0xFFFFFF), None, 2);
    unsafe { BR_CLOSE = (cx, cy, btn, btn) };

    // [<] geri.
    let bx = cx - btn - 8;
    gfx::fill_rect(bx, cy, btn, btn, gfx::rgb(C_BTN));
    text(bx + 4, cy + 3, "<", gfx::rgb(0xFFFFFF), None, 2);
    unsafe { BR_BACK = (bx, cy, btn, btn) };

    // Adres çubuğu satırı.
    let ay = wy + title_h + 4;
    let go_w = 4 * 16 + 8;
    let updown_w = 24usize;
    let ax = wx + 8;
    let aw = ww - 16 - go_w - 8 - 2 * (updown_w + 4);
    gfx::fill_rect(ax, ay, aw, addr_h - 8, gfx::rgb(C_ADDR_BG));
    gfx::rect(ax, ay, aw, addr_h - 8, gfx::rgb(C_WIN_BORDER));
    unsafe { ADDR_RECT = (ax, ay, aw, addr_h - 8) };

    // [Git] düğmesi.
    let gx = ax + aw + 8;
    gfx::fill_rect(gx, ay, go_w, addr_h - 8, gfx::rgb(C_BTN));
    text(gx + 4, ay + 3, "Git", gfx::rgb(0xFFFFFF), None, 2);
    unsafe { BR_GO = (gx, ay, go_w, addr_h - 8) };

    // Kaydırma [▲] [▼] (yukarı/aşağı, metin: ^ v).
    let ux = gx + go_w + 6;
    gfx::fill_rect(ux, ay, updown_w, addr_h - 8, gfx::rgb(C_BTN));
    text(ux + 4, ay + 3, "^", gfx::rgb(0xFFFFFF), None, 2);
    unsafe { BR_UP = (ux, ay, updown_w, addr_h - 8) };
    let dx = ux + updown_w + 4;
    gfx::fill_rect(dx, ay, updown_w, addr_h - 8, gfx::rgb(C_BTN));
    text(dx + 4, ay + 3, "v", gfx::rgb(0xFFFFFF), None, 2);
    unsafe { BR_DOWN = (dx, ay, updown_w, addr_h - 8) };

    // İçerik alanı geometrisi.
    let pad = 10usize;
    let cont_x = wx + pad;
    let cont_y = wy + title_h + addr_h + pad;
    let cont_w = ww - 2 * pad;
    let cont_h = (wy + wh) - cont_y - pad;
    unsafe {
        CONT_X = cont_x;
        CONT_Y = cont_y;
        CONT_COLS = cont_w / 8;
        CONT_ROWS = cont_h / LINE_H;
    }

    draw_addrbar();
}

/// Adres çubuğunun metnini (kaydırarak) çizer.
fn draw_addrbar() {
    let (ax, ay, aw, ah) = unsafe { ADDR_RECT };
    gfx::fill_rect(ax + 1, ay + 1, aw - 2, ah - 2, gfx::rgb(C_ADDR_BG));
    let url = current_url();
    let cap = (aw - 8) / 8;
    let shown: &str = if url.len() > cap.saturating_sub(1) {
        &url[url.len() - cap.saturating_sub(1)..]
    } else {
        url
    };
    text(ax + 4, ay + 4, shown, gfx::rgb(0xDDEBF5), None, 1);
    // Yanıp sönmeyen basit imleç.
    let cx = ax + 4 + shown.len() * 8;
    if cx + 6 < ax + aw {
        gfx::fill_rect(cx, ay + 4, 6, 8, gfx::rgb(0x9FCBEA));
    }
}

fn current_url() -> &'static str {
    unsafe { core::str::from_utf8(&URL_BUF[..URL_LEN]).unwrap_or("") }
}

fn set_url(s: &str) {
    let b = s.as_bytes();
    let n = core::cmp::min(b.len(), URL_MAX);
    unsafe {
        URL_BUF[..n].copy_from_slice(&b[..n]);
        URL_LEN = n;
    }
}

/// Başlangıç (ana) sayfası: birkaç tıklanabilir yer imi.
fn set_home_page() {
    let mut text = String::new();
    let mut links: Vec<String> = Vec::new();
    let mut spans: Vec<(u32, u32, u32)> = Vec::new();
    text.push_str("MinOS Tarayici\n\n");
    text.push_str("Adres cubuguna bir adres yazip Git'e (ya da Enter) basin.\n");
    text.push_str("Asagidaki baglantilara tiklayabilirsiniz:\n\n");
    for (label, url) in [
        ("example.com", "https://example.com"),
        ("Wikipedia", "https://en.wikipedia.org/wiki/Operating_system"),
        ("Hacker News", "https://news.ycombinator.com"),
        ("info.cern.ch (ilk web sayfasi)", "http://info.cern.ch"),
    ] {
        let start = text.len() as u32;
        text.push_str(label);
        let end = text.len() as u32;
        spans.push((start, end, links.len() as u32));
        links.push(url.to_string());
        text.push('\n');
    }
    unsafe {
        PAGE = Some(Page {
            text,
            links,
            spans,
            title: "Ana Sayfa".to_string(),
        });
        SCROLL = 0;
    }
}

/// Verilen URL'yi yükler: çözer, çeker, yönlendirmeleri izler, ayrıştırır.
fn browser_load(url: &str) {
    set_url(url);
    draw_addrbar();
    // "Yükleniyor" göster.
    let (cont_x, cont_y) = unsafe { (CONT_X, CONT_Y) };
    let (wx, wy, ww, wh) = unsafe { BWIN };
    gfx::fill_rect(
        cont_x,
        cont_y,
        ww - 20,
        (wy + wh) - cont_y - 10,
        gfx::rgb(C_WIN_BODY),
    );
    text(cont_x, cont_y, "Yukleniyor...", gfx::rgb(0xF1C40F), None, 2);
    let _ = (wx,);
    present_cursor();

    let (mut https, mut host, mut path) = split_url(url);

    let mut result: Result<String, &'static str> = Err("baglanti yok");
    for _ in 0..6 {
        result = if https {
            crate::net::fetch_https(&host, &path)
        } else {
            crate::net::fetch(&host, &path)
        };
        let resp = match &result {
            Ok(r) => r,
            Err(_) => break,
        };
        let code = status_code(resp);
        if (300..400).contains(&code) {
            if let Some(loc) = header_value(resp, "location") {
                let abs = resolve(&loc, https, &host, &path);
                if let Some(a) = abs {
                    let (h2, ho2, pa2) = split_url(&a);
                    https = h2;
                    host = ho2;
                    path = pa2;
                    continue;
                }
            }
        }
        break;
    }

    unsafe {
        CUR_HTTPS = https;
        CUR_HOST = host.clone();
        CUR_PATH = path.clone();
        SCROLL = 0;
    }
    set_url(&rebuild_url(https, &host, &path));
    draw_addrbar();

    match result {
        Ok(resp) => parse_page(&resp, https, &host, &path),
        Err(e) => {
            let mut text = String::new();
            text.push_str("Sayfa yuklenemedi:\n\n");
            text.push_str(e);
            unsafe {
                PAGE = Some(Page {
                    text,
                    links: Vec::new(),
                    spans: Vec::new(),
                    title: "Hata".to_string(),
                });
            }
        }
    }
    browser_render();
}

/// Geçmişe ekleyip yeni adrese gider.
fn navigate(url: &str) {
    let cur = current_url();
    if !cur.is_empty() {
        unsafe {
            let h = &mut *core::ptr::addr_of_mut!(HIST);
            h.push(cur.to_string());
            if h.len() > 32 {
                h.remove(0);
            }
        }
    }
    browser_load(url);
}

fn go_back() {
    let prev = unsafe {
        let h = &mut *core::ptr::addr_of_mut!(HIST);
        h.pop()
    };
    if let Some(u) = prev {
        browser_load(&u);
    }
}

/// İçerik alanını kelime sarmalı çizer; bağlantıları renklendirir ve
/// tıklanabilir dikdörtgenlerini kaydeder.
fn browser_render() {
    let (cont_x, cont_y, cols, rows) = unsafe { (CONT_X, CONT_Y, CONT_COLS, CONT_ROWS) };
    let (wx, wy, ww, wh) = unsafe { BWIN };
    let _ = wx;
    // İçerik arkaplanını temizle.
    gfx::fill_rect(
        cont_x,
        cont_y,
        ww - 20,
        (wy + wh) - cont_y - 10,
        gfx::rgb(C_WIN_BODY),
    );

    unsafe { LINK_RECT_COUNT = 0 };
    let scroll = unsafe { SCROLL };

    let page = unsafe { &*core::ptr::addr_of!(PAGE) };
    let page = match page {
        Some(p) => p,
        None => return,
    };

    let fg = gfx::rgb(C_WIN_TXT);
    let link_col = gfx::rgb(C_LINK);
    let bg = Some(gfx::rgb(C_WIN_BODY));

    let bytes = page.text.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;
    let mut col = 0usize;
    let mut row = 0usize;

    while i < n {
        let c = bytes[i];
        if c == b'\n' {
            col = 0;
            row += 1;
            i += 1;
            continue;
        }
        if c == b' ' {
            if col < cols {
                col += 1;
            }
            i += 1;
            continue;
        }
        // Bir kelime (boşluk/yeni satır olmayan ardışık karakterler).
        let wstart = i;
        while i < n && bytes[i] != b' ' && bytes[i] != b'\n' {
            i += 1;
        }
        let word = core::str::from_utf8(&bytes[wstart..i]).unwrap_or("");
        let wlen = word.chars().count();

        // Satır sığmazsa alt satıra geç.
        if col + wlen > cols && col > 0 {
            col = 0;
            row += 1;
        }

        // Bu kelime bir bağlantı mı?
        let link_idx = span_at(&page.spans, wstart as u32);

        // Görünür satır penceresindeyse çiz.
        if row >= scroll && row < scroll + rows {
            let px = cont_x + col * 8;
            let py = cont_y + (row - scroll) * LINE_H;
            let color = if link_idx.is_some() { link_col } else { fg };
            // Kelime sığacak kadar (taşarsa kırp) çiz.
            let mut dx = px;
            let mut drawn = 0usize;
            for ch in word.chars() {
                if col + drawn >= cols {
                    break;
                }
                draw_glyph(dx, py, ch, color, bg, 1);
                dx += 8;
                drawn += 1;
            }
            if let Some(li) = link_idx {
                push_link_rect(px, py, drawn * 8, LINE_H, li as usize);
            }
        }
        col += wlen;
    }

    unsafe { TOTAL_LINES = row + 1 };
    draw_browser_title();
}

/// Başlık çubuğunda "Internet — <sayfa başlığı>" yazısını günceller.
fn draw_browser_title() {
    let (wx, wy, _, _) = unsafe { BWIN };
    let (bx, _, _, _) = unsafe { BR_BACK };
    let title_h = 28usize;
    let strip_w = bx.saturating_sub(wx + 12);
    gfx::fill_rect(wx + 1, wy + 1, strip_w, title_h - 2, gfx::rgb(C_WIN_TITLE));

    let mut s = String::from("Internet");
    let t = unsafe {
        (*core::ptr::addr_of!(PAGE))
            .as_ref()
            .map(|p| p.title.clone())
            .unwrap_or_default()
    };
    if !t.is_empty() {
        s.push_str("  -  ");
        s.push_str(&t);
    }
    let maxc = strip_w / 16;
    let shown: String = s.chars().take(maxc).collect();
    text(wx + 10, wy + 6, &shown, gfx::rgb(0xFFFFFF), None, 2);
}

fn push_link_rect(x: usize, y: usize, w: usize, h: usize, idx: usize) {
    unsafe {
        if LINK_RECT_COUNT < LINK_RECTS_MAX {
            LINK_RECTS[LINK_RECT_COUNT] = (x, y, w, h, idx);
            LINK_RECT_COUNT += 1;
        }
    }
}

/// Verilen bayt ofseti bir bağlantı aralığına düşüyorsa link indeksini döndürür.
fn span_at(spans: &[(u32, u32, u32)], off: u32) -> Option<u32> {
    for &(s, e, idx) in spans {
        if off >= s && off < e {
            return Some(idx);
        }
    }
    None
}

fn scroll_by(delta: isize) {
    unsafe {
        let max = TOTAL_LINES.saturating_sub(CONT_ROWS);
        let cur = SCROLL as isize + delta;
        SCROLL = cur.clamp(0, max as isize) as usize;
    }
    browser_render();
    present_cursor();
}

// --- URL yardımcıları ---

fn split_url(url: &str) -> (bool, String, String) {
    let (https, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        (false, url)
    };
    match rest.find('/') {
        Some(i) => (https, rest[..i].to_string(), rest[i..].to_string()),
        None => (https, rest.to_string(), "/".to_string()),
    }
}

fn rebuild_url(https: bool, host: &str, path: &str) -> String {
    let scheme = if https { "https://" } else { "http://" };
    format!("{scheme}{host}{path}")
}

/// Göreli/mutlak bir href'i mutlak URL'ye çözer. Atlanması gerekenlerde None.
fn resolve(href: &str, https: bool, host: &str, path: &str) -> Option<String> {
    let h = href.trim();
    if h.is_empty()
        || h.starts_with('#')
        || h.starts_with("javascript:")
        || h.starts_with("mailto:")
        || h.starts_with("tel:")
        || h.starts_with("data:")
    {
        return None;
    }
    if h.starts_with("http://") || h.starts_with("https://") {
        return Some(h.to_string());
    }
    let scheme = if https { "https://" } else { "http://" };
    if let Some(rest) = h.strip_prefix("//") {
        return Some(format!("{scheme}{rest}"));
    }
    if h.starts_with('/') {
        return Some(format!("{scheme}{host}{h}"));
    }
    let dir = match path.rfind('/') {
        Some(i) => &path[..i + 1],
        None => "/",
    };
    Some(format!("{scheme}{host}{dir}{h}"))
}

fn status_code(resp: &str) -> u16 {
    resp.lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .unwrap_or(0)
}

fn header_value(resp: &str, name: &str) -> Option<String> {
    for line in resp.lines() {
        if line.is_empty() {
            break;
        }
        if let Some(colon) = line.find(':') {
            let (k, v) = line.split_at(colon);
            if k.eq_ignore_ascii_case(name) {
                return Some(v[1..].trim().to_string());
            }
        }
    }
    None
}

// --- HTML ayrıştırıcı: görünür metin + bağlantılar ---

fn parse_page(resp: &str, https: bool, host: &str, path: &str) {
    let body = match resp.find("\r\n\r\n") {
        Some(i) => &resp[i + 4..],
        None => resp,
    };

    let mut text = String::new();
    let mut links: Vec<String> = Vec::new();
    let mut spans: Vec<(u32, u32, u32)> = Vec::new();
    let mut title = String::new();
    let mut in_title = false;
    let mut cur_link: Option<u32> = None;
    let mut link_start: u32 = 0;
    let mut last_space = true;

    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag = String::new();
            for nc in chars.by_ref() {
                if nc == '>' {
                    break;
                }
                tag.push(nc);
            }
            let name = tag_name(&tag);
            match name.as_str() {
                "script" | "style" => {
                    let close = if name == "script" { "/script" } else { "/style" };
                    // Kapanış etiketine kadar atla.
                    'skip: while let Some(sc) = chars.next() {
                        if sc == '<' {
                            let mut t2 = String::new();
                            for nc in chars.by_ref() {
                                if nc == '>' {
                                    break;
                                }
                                t2.push(nc);
                            }
                            if tag_name(&t2) == close {
                                break 'skip;
                            }
                        }
                    }
                }
                "title" => in_title = true,
                "/title" => in_title = false,
                "a" => {
                    if let Some(href) = extract_href(&tag) {
                        if let Some(abs) = resolve(&href, https, host, path) {
                            cur_link = Some(links.len() as u32);
                            links.push(abs);
                            link_start = text.len() as u32;
                        }
                    }
                }
                "/a" => {
                    if let Some(li) = cur_link.take() {
                        let end = text.len() as u32;
                        if end > link_start {
                            spans.push((link_start, end, li));
                        }
                    }
                }
                _ => {
                    if is_block(&name) && !text.ends_with('\n') {
                        text.push('\n');
                        last_space = true;
                    }
                }
            }
            continue;
        }

        if c == '&' {
            let mut ent = String::new();
            while let Some(&nc) = chars.peek() {
                if nc == ';' {
                    chars.next();
                    break;
                }
                if ent.len() >= 10 || nc == '<' || nc == '&' || nc.is_whitespace() {
                    break;
                }
                ent.push(nc);
                chars.next();
            }
            if let Some(ch) = decode_entity(&ent) {
                push_visible(ch, &mut text, &mut title, in_title, &mut last_space);
            }
            continue;
        }

        push_visible(c, &mut text, &mut title, in_title, &mut last_space);
    }

    if let Some(li) = cur_link.take() {
        let end = text.len() as u32;
        if end > link_start {
            spans.push((link_start, end, li));
        }
    }

    unsafe {
        PAGE = Some(Page {
            text,
            links,
            spans,
            title,
        });
    }
}

fn push_visible(ch: char, text: &mut String, title: &mut String, in_title: bool, last_space: &mut bool) {
    if in_title {
        if ch.is_whitespace() {
            if !title.is_empty() && !title.ends_with(' ') {
                title.push(' ');
            }
        } else {
            title.push(ch);
        }
        return;
    }
    if ch.is_whitespace() {
        if !*last_space {
            text.push(' ');
            *last_space = true;
        }
    } else {
        text.push(ch);
        *last_space = false;
    }
}

fn decode_entity(ent: &str) -> Option<char> {
    Some(match ent {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" | "#39" => '\'',
        "nbsp" | "#160" => ' ',
        "mdash" | "ndash" | "#8211" | "#8212" => '-',
        "hellip" => '.',
        "copy" => 'c',
        "reg" => 'r',
        "trade" => 't',
        _ => {
            // &#NNN; sayısal (yalnızca ASCII aralığı).
            if let Some(num) = ent.strip_prefix('#') {
                if let Ok(v) = num.parse::<u32>() {
                    if (32..127).contains(&v) {
                        return char::from_u32(v);
                    }
                }
            }
            return None;
        }
    })
}

fn tag_name(tag: &str) -> String {
    let mut s = String::new();
    for ch in tag.trim_start().chars() {
        if ch == '/' || ch.is_ascii_alphanumeric() {
            s.push(ch.to_ascii_lowercase());
        } else {
            break;
        }
    }
    s
}

fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "/p"
            | "br"
            | "div"
            | "/div"
            | "li"
            | "/li"
            | "tr"
            | "/tr"
            | "ul"
            | "/ul"
            | "ol"
            | "/ol"
            | "h1"
            | "/h1"
            | "h2"
            | "/h2"
            | "h3"
            | "/h3"
            | "h4"
            | "/h4"
            | "h5"
            | "/h5"
            | "h6"
            | "/h6"
            | "table"
            | "/table"
            | "hr"
            | "header"
            | "/header"
            | "footer"
            | "section"
            | "article"
            | "/article"
            | "nav"
            | "blockquote"
    )
}

fn extract_href(tag: &str) -> Option<String> {
    let lt = tag.to_ascii_lowercase();
    let p = lt.find("href")?;
    let rest = &tag[p + 4..];
    let eq = rest.find('=')?;
    let v = rest[eq + 1..].trim_start();
    let val = if let Some(v) = v.strip_prefix('"') {
        &v[..v.find('"').unwrap_or(v.len())]
    } else if let Some(v) = v.strip_prefix('\'') {
        &v[..v.find('\'').unwrap_or(v.len())]
    } else {
        &v[..v.find(|c: char| c.is_whitespace()).unwrap_or(v.len())]
    };
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

/// Tarayıcı penceresindeki tıklamaları işler. İşlendiyse true döner.
fn browser_click(mx: usize, my: usize) {
    if inside(mx, my, unsafe { BR_CLOSE }) {
        close_browser();
        return;
    }
    if inside(mx, my, unsafe { BR_BACK }) {
        go_back();
        return;
    }
    if inside(mx, my, unsafe { BR_GO }) {
        let url = current_url().to_string();
        if !url.is_empty() {
            navigate(&url);
            present_cursor();
        }
        return;
    }
    if inside(mx, my, unsafe { BR_UP }) {
        let step = unsafe { CONT_ROWS } as isize / 2;
        scroll_by(-step.max(1));
        return;
    }
    if inside(mx, my, unsafe { BR_DOWN }) {
        let step = unsafe { CONT_ROWS } as isize / 2;
        scroll_by(step.max(1));
        return;
    }
    // Bağlantılar.
    let count = unsafe { LINK_RECT_COUNT };
    for k in 0..count {
        let (x, y, w, h, idx) = unsafe { LINK_RECTS[k] };
        if inside(mx, my, (x, y, w, h)) {
            let url = unsafe {
                let page = &*core::ptr::addr_of!(PAGE);
                page.as_ref().and_then(|p| p.links.get(idx)).cloned()
            };
            if let Some(u) = url {
                navigate(&u);
                present_cursor();
            }
            return;
        }
    }
}

/// Tarayıcı açıkken klavye karakterini işler.
fn browser_key(c: char) {
    if c == '\n' || c == '\r' {
        let url = current_url().to_string();
        if !url.is_empty() {
            navigate(&url);
            present_cursor();
        }
        return;
    }
    if c == '\u{8}' {
        unsafe {
            // Son UTF-8 karakterini sil (basitçe son baytı).
            if URL_LEN > 0 {
                URL_LEN -= 1;
                // Devam baytlarını da temizle.
                while URL_LEN > 0 && (URL_BUF[URL_LEN] & 0xC0) == 0x80 {
                    URL_LEN -= 1;
                }
            }
        }
        draw_addrbar();
        present_cursor();
        return;
    }
    if (c as u32) >= 0x20 && (c as u32) < 0xE000 && (c as u32) != 0x7F {
        let mut tmp = [0u8; 4];
        let s = c.encode_utf8(&mut tmp);
        let b = s.as_bytes();
        unsafe {
            if URL_LEN + b.len() <= URL_MAX {
                URL_BUF[URL_LEN..URL_LEN + b.len()].copy_from_slice(b);
                URL_LEN += b.len();
            }
        }
        draw_addrbar();
        present_cursor();
    }
}

/// Masaüstü döngüsü. F1/ESC ile terminale dönünce çıkar.
pub fn run() {
    unsafe { GUI_DIR = fs::ROOT };
    draw_desktop();
    present_cursor();

    let mut prev_left = false;
    let mut last_x = mouse::x();
    let mut last_y = mouse::y();
    let mut last_sec = 0xFFu8;

    loop {
        input::poll();

        // Klavye olayları.
        while let Some(c) = keyboard::pop() {
            if c == KEY_TOGGLE {
                return;
            }
            if unsafe { EDITOR_OPEN } {
                if c == KEY_ESC {
                    close_editor();
                } else {
                    editor_key(c);
                }
            } else if unsafe { BROWSER_OPEN } {
                if c == KEY_ESC {
                    close_browser();
                } else {
                    browser_key(c);
                }
            } else if c == KEY_ESC {
                return;
            }
        }

        // Canlı saat: saniye değişince üst barı tazele (imleci koruyarak).
        let sec = crate::rtc::second();
        if sec != last_sec {
            last_sec = sec;
            restore_under();
            draw_clock();
            present_cursor();
        }

        // Fare hareketi.
        let mx = mouse::x();
        let my = mouse::y();
        if mx != last_x || my != last_y {
            move_cursor(mx, my);
            last_x = mx;
            last_y = my;
        }

        // Sol tık (basma kenarı).
        let left = mouse::left();
        if left && !prev_left {
            handle_click(mx, my);
            last_x = mouse::x();
            last_y = mouse::y();
        }
        prev_left = left;

        core::hint::spin_loop();
    }
}

// --- Tampona sayı/metin yazma yardımcıları (heap yok) ---

fn put_str(dst: &mut [u8], s: &str) -> usize {
    let b = s.as_bytes();
    let n = core::cmp::min(b.len(), dst.len());
    dst[..n].copy_from_slice(&b[..n]);
    n
}

fn put_num(dst: &mut [u8], mut n: u32) -> usize {
    if n == 0 {
        if !dst.is_empty() {
            dst[0] = b'0';
            return 1;
        }
        return 0;
    }
    let mut tmp = [0u8; 10];
    let mut i = 0;
    while n > 0 {
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    let mut w = 0;
    while i > 0 && w < dst.len() {
        i -= 1;
        dst[w] = tmp[i];
        w += 1;
    }
    w
}
