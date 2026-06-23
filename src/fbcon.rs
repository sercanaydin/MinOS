//! Framebuffer (grafik modu) metin konsolu.
//!
//! Grafik modda VGA metin tamponu (0xB8000) görünmez; bu yüzden yazıları
//! gömülü 8x8 fontla doğrudan framebuffer'a çizeriz. Hücreleri bir ızgarada
//! tutarız; böylece kaydırma yapabilir ve masaüstünden terminale dönünce
//! içeriği yeniden çizebiliriz.
//!
//! `vga::Color` paletini kullanır; böylece mevcut `print!/println!` çağrıları
//! grafik modda da aynı renklerle çalışır.

use crate::font_data;
use crate::gfx;
use crate::vga::Color;

const SCALE: usize = 2; // 8x8 font -> 16x16 hücre
const CW: usize = 8 * SCALE;
const CH: usize = 8 * SCALE;
const MAX_COLS: usize = 128;
const MAX_ROWS: usize = 64;

/// 16 renklik standart VGA paleti (0x00RRGGBB).
static PALETTE: [u32; 16] = [
    0x000000, 0x0000AA, 0x00AA00, 0x00AAAA, 0xAA0000, 0xAA00AA, 0xAA5500, 0xAAAAAA, 0x555555,
    0x5555FF, 0x55FF55, 0x55FFFF, 0xFF5555, 0xFF55FF, 0xFFFF55, 0xFFFFFF,
];

#[derive(Clone, Copy)]
struct Cell {
    ch: char,
    fg: u8,
    bg: u8,
}

struct Console {
    cols: usize,
    rows: usize,
    cx: usize,
    cy: usize,
    fg: u8,
    bg: u8,
    pcx: usize,
    pcy: usize,
    grid: [[Cell; MAX_COLS]; MAX_ROWS],
}

static mut CON: Console = Console {
    cols: 0,
    rows: 0,
    cx: 0,
    cy: 0,
    fg: Color::LightGray as u8,
    bg: Color::Black as u8,
    pcx: 0,
    pcy: 0,
    grid: [[Cell {
        ch: ' ',
        fg: Color::LightGray as u8,
        bg: Color::Black as u8,
    }; MAX_COLS]; MAX_ROWS],
};

#[allow(static_mut_refs)]
fn con() -> &'static mut Console {
    unsafe { &mut CON }
}

/// Grafik mod konsolunu başlatır (boyutları framebuffer'dan alır, temizler).
pub fn init() {
    let c = con();
    c.cols = core::cmp::min(gfx::width() / CW, MAX_COLS);
    c.rows = core::cmp::min(gfx::height() / CH, MAX_ROWS);
    c.fg = Color::LightGray as u8;
    c.bg = Color::Black as u8;
    clear();
}

/// Tek bir glifi (px, py) noktasına, verilen ön/arka renklerle çizer.
fn draw_glyph(px: usize, py: usize, c: char, fg: u32, bg: u32) {
    let g = font_data::glyph(c);
    for (ry, bits) in g.iter().enumerate() {
        for cx in 0..8 {
            let on = (bits >> cx) & 1 != 0;
            let color = if on { fg } else { bg };
            gfx::fill_rect(px + cx * SCALE, py + ry * SCALE, SCALE, SCALE, color);
        }
    }
}

/// Izgaradaki bir hücreyi (içeriğine göre) yeniden çizer.
fn draw_cell(r: usize, col: usize) {
    let c = con();
    let cell = c.grid[r][col];
    draw_glyph(
        col * CW,
        r * CH,
        cell.ch,
        gfx::rgb(PALETTE[cell.fg as usize]),
        gfx::rgb(PALETTE[cell.bg as usize]),
    );
}

fn erase_cursor() {
    let c = con();
    if c.pcy < c.rows && c.pcx < c.cols {
        draw_cell(c.pcy, c.pcx);
    }
}

fn draw_cursor() {
    let c = con();
    let fg = gfx::rgb(PALETTE[c.fg as usize]);
    // Alt çizgi imleci.
    gfx::fill_rect(c.cx * CW, c.cy * CH + CH - SCALE, CW, SCALE, fg);
    c.pcx = c.cx;
    c.pcy = c.cy;
}

fn update_cursor() {
    erase_cursor();
    draw_cursor();
}

pub fn set_color(fg: Color, bg: Color) {
    let c = con();
    c.fg = fg as u8;
    c.bg = bg as u8;
}

pub fn clear() {
    let c = con();
    let blank = Cell {
        ch: ' ',
        fg: c.fg,
        bg: c.bg,
    };
    for r in 0..c.rows {
        for col in 0..c.cols {
            c.grid[r][col] = blank;
        }
    }
    gfx::clear(gfx::rgb(PALETTE[c.bg as usize]));
    c.cx = 0;
    c.cy = 0;
    c.pcx = 0;
    c.pcy = 0;
    draw_cursor();
}

fn new_line() {
    let c = con();
    c.cx = 0;
    if c.cy + 1 < c.rows {
        c.cy += 1;
    } else {
        scroll();
    }
}

fn scroll() {
    let c = con();
    for r in 1..c.rows {
        for col in 0..c.cols {
            c.grid[r - 1][col] = c.grid[r][col];
        }
    }
    let blank = Cell {
        ch: ' ',
        fg: c.fg,
        bg: c.bg,
    };
    for col in 0..c.cols {
        c.grid[c.rows - 1][col] = blank;
    }
    // Tüm ekranı yeniden çiz (kaydırma sonrası).
    for r in 0..c.rows {
        for col in 0..c.cols {
            draw_cell(r, col);
        }
    }
    c.cy = c.rows - 1;
}

fn backspace() {
    let c = con();
    if c.cx > 0 {
        c.cx -= 1;
    } else if c.cy > 0 {
        c.cy -= 1;
        c.cx = c.cols - 1;
    }
    c.grid[c.cy][c.cx] = Cell {
        ch: ' ',
        fg: c.fg,
        bg: c.bg,
    };
    draw_cell(c.cy, c.cx);
}

pub fn write_char(ch: char) {
    match ch {
        '\n' => new_line(),
        '\u{8}' => backspace(),
        '\r' => {}
        _ => {
            let c = con();
            if c.cols == 0 || c.rows == 0 {
                return;
            }
            if c.cx >= c.cols {
                new_line();
            }
            let c = con();
            c.grid[c.cy][c.cx] = Cell {
                ch,
                fg: c.fg,
                bg: c.bg,
            };
            draw_cell(c.cy, c.cx);
            c.cx += 1;
        }
    }
    update_cursor();
}

pub fn write_str(s: &str) {
    for ch in s.chars() {
        write_char(ch);
    }
}

/// Tüm konsolu (ızgaradaki içeriği) framebuffer'a yeniden çizer.
/// Masaüstünden terminale dönünce çağrılır.
pub fn redraw() {
    let c = con();
    gfx::clear(gfx::rgb(PALETTE[c.bg as usize]));
    for r in 0..c.rows {
        for col in 0..c.cols {
            draw_cell(r, col);
        }
    }
    c.pcx = c.cx;
    c.pcy = c.cy;
    draw_cursor();
}
