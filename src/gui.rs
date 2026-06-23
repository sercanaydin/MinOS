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
    let hint = "[F1]/[ESC] terminal";
    let hx = w.saturating_sub(hint.chars().count() * 16 + 12);
    text(hx, 9, hint, gfx::rgb(0xCfe0f0), None, 2);
    draw_clock();
}

/// Üst bardaki saat/tarih alanının (x, genişlik) değerleri — ortalanmış.
fn clock_region() -> (usize, usize) {
    let chars = 20; // "GG.AA.YYYY  SS:DD:ss"
    let cw = chars * 16; // ölçek 2 → karakter 16px
    let x = gfx::width().saturating_sub(cw) / 2;
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
    unsafe {
        if WIN_OPEN {
            if inside(mx, my, CLOSE_BTN) {
                draw_desktop();
                present_cursor();
            }
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
                        open_file(ic.file);
                        present_cursor();
                    }
                }
                return;
            }
        }
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
            if c == KEY_TOGGLE || c == KEY_ESC {
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
