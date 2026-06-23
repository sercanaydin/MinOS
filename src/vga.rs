//! VGA metin modu sürücüsü.
//!
//! BIOS bizi 80x25 renkli metin modunda bırakır. Ekran belleği 0xB8000
//! adresindedir ve her karakter 2 bayttır: [ASCII bayt][renk baytı].

use core::fmt;

const VGA_BUFFER: *mut u8 = 0xB8000 as *mut u8;
const WIDTH: usize = 80;
const HEIGHT: usize = 25;

/// 4-bit VGA renkleri.
#[allow(dead_code)]
#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

/// Ön plan + arka plandan bir renk baytı üretir.
const fn color_byte(fg: Color, bg: Color) -> u8 {
    (bg as u8) << 4 | (fg as u8)
}

pub struct Writer {
    col: usize,
    row: usize,
    color: u8,
}

impl Writer {
    /// Ekrandaki tek bir hücreyi yazar.
    fn put_cell(&self, row: usize, col: usize, ch: u8, color: u8) {
        let offset = (row * WIDTH + col) * 2;
        unsafe {
            core::ptr::write_volatile(VGA_BUFFER.add(offset), ch);
            core::ptr::write_volatile(VGA_BUFFER.add(offset + 1), color);
        }
    }

    /// Tek bir karakteri ekrana basar; '\n' ve Backspace özel işlenir.
    /// Türkçe/Unicode karakterler VGA kod sayfası baytına çevrilir.
    pub fn write_char(&mut self, c: char) {
        match c {
            '\n' => self.new_line(),
            '\u{8}' => self.backspace(),
            _ => {
                if self.col >= WIDTH {
                    self.new_line();
                }
                self.put_cell(self.row, self.col, map_char(c), self.color);
                self.col += 1;
            }
        }
        self.update_cursor();
    }

    fn backspace(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = WIDTH - 1;
        }
        self.put_cell(self.row, self.col, b' ', self.color);
    }

    fn new_line(&mut self) {
        self.col = 0;
        if self.row + 1 < HEIGHT {
            self.row += 1;
        } else {
            self.scroll();
        }
    }

    /// Tüm satırları bir yukarı kaydırır, en alt satırı temizler.
    fn scroll(&mut self) {
        for row in 1..HEIGHT {
            for col in 0..WIDTH {
                let from = (row * WIDTH + col) * 2;
                let to = ((row - 1) * WIDTH + col) * 2;
                unsafe {
                    let ch = core::ptr::read_volatile(VGA_BUFFER.add(from));
                    let cl = core::ptr::read_volatile(VGA_BUFFER.add(from + 1));
                    core::ptr::write_volatile(VGA_BUFFER.add(to), ch);
                    core::ptr::write_volatile(VGA_BUFFER.add(to + 1), cl);
                }
            }
        }
        for col in 0..WIDTH {
            self.put_cell(HEIGHT - 1, col, b' ', self.color);
        }
        self.row = HEIGHT - 1;
    }

    pub fn clear(&mut self) {
        for row in 0..HEIGHT {
            for col in 0..WIDTH {
                self.put_cell(row, col, b' ', self.color);
            }
        }
        self.row = 0;
        self.col = 0;
        self.update_cursor();
    }

    pub fn set_color(&mut self, fg: Color, bg: Color) {
        self.color = color_byte(fg, bg);
    }

    /// Donanım imlecini (yanıp sönen alt çizgi) geçerli konuma taşır.
    fn update_cursor(&self) {
        let pos = (self.row * WIDTH + self.col) as u16;
        unsafe {
            crate::port::outb(0x3D4, 0x0F);
            crate::port::outb(0x3D5, (pos & 0xFF) as u8);
            crate::port::outb(0x3D4, 0x0E);
            crate::port::outb(0x3D5, (pos >> 8) as u8);
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            self.write_char(c);
        }
        Ok(())
    }
}

/// Bir Unicode karakteri VGA ekran kodu baytına çevirir.
/// ASCII olduğu gibi geçer; Türkçe harfler fonta yüklediğimiz kodlara
/// eşlenir; bilinmeyenler '?' olur.
pub fn map_char(c: char) -> u8 {
    use crate::font;
    match c {
        ' '..='~' => c as u8,
        '\n' => b'\n',
        'ç' => font::CH_C_CED_L,
        'Ç' => font::CH_C_CED_U,
        'ş' => font::CH_S_CED_L,
        'Ş' => font::CH_S_CED_U,
        'ğ' => font::CH_G_BRV_L,
        'Ğ' => font::CH_G_BRV_U,
        'ı' => font::CH_I_DOTLESS,
        'İ' => font::CH_I_DOTTED_U,
        'ö' => font::CH_O_DIA_L,
        'Ö' => font::CH_O_DIA_U,
        'ü' => font::CH_U_DIA_L,
        'Ü' => font::CH_U_DIA_U,
        _ => b'?',
    }
}

// Çekirdek tek iş parçacıklı ve kesmesiz çalıştığı için tek bir global
// Writer'ı doğrudan kullanmak güvenlidir.
static mut WRITER: Writer = Writer {
    col: 0,
    row: 0,
    color: color_byte(Color::LightGray, Color::Black),
};

/// Global Writer'a güvenli olmayan referans döndürür.
#[allow(static_mut_refs)]
fn writer() -> &'static mut Writer {
    unsafe { &mut WRITER }
}

pub fn clear() {
    if crate::gfx::is_active() {
        crate::fbcon::clear();
        return;
    }
    writer().clear();
}


pub fn set_color(fg: Color, bg: Color) {
    if crate::gfx::is_active() {
        crate::fbcon::set_color(fg, bg);
        return;
    }
    writer().set_color(fg, bg);
}

/// Grafik modda metni framebuffer konsoluna yönlendiren köprü.
struct FbWriter;

impl fmt::Write for FbWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        crate::fbcon::write_str(s);
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    if crate::gfx::is_active() {
        FbWriter.write_fmt(args).ok();
        return;
    }
    writer().write_fmt(args).ok();
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::vga::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
