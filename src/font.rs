//! VGA metin modu fontuna Türkçe harfleri ekler.
//!
//! Standart VGA (CP437) fontunda `ç ö ü` vardır ama `ş ğ ı İ Ş Ğ` yoktur.
//! Tutarlı görünmesi için bütün Türkçe harfleri, fonttaki temel harflerden
//! (s, g, i, o, u, c ...) okuyup üzerlerine aksan (çengel/yay/iki nokta)
//! ekleyerek çalışma anında üretir ve kullanılmayan kod noktalarına yazarız.
//!
//! VGA fontu, bellek "plane 2" içinde saklanır; oraya erişmek için
//! sequencer/graphics denetleyici yazmaçlarını geçici olarak değiştiririz.

use crate::port::outb;

const FONT_MEM: *mut u8 = 0xA0000 as *mut u8;
const BYTES_PER_GLYPH: usize = 32; // 16 satır kullanılır, kalanı dolgu

// Türkçe harflerin yazılacağı (CP437'de kullanmadığımız) kod noktaları.
pub const CH_C_CED_L: u8 = 0x80; // ç
pub const CH_C_CED_U: u8 = 0x81; // Ç
pub const CH_S_CED_L: u8 = 0x82; // ş
pub const CH_S_CED_U: u8 = 0x83; // Ş
pub const CH_G_BRV_L: u8 = 0x84; // ğ
pub const CH_G_BRV_U: u8 = 0x85; // Ğ
pub const CH_I_DOTLESS: u8 = 0x86; // ı
pub const CH_I_DOTTED_U: u8 = 0x87; // İ
pub const CH_O_DIA_L: u8 = 0x88; // ö
pub const CH_O_DIA_U: u8 = 0x89; // Ö
pub const CH_U_DIA_L: u8 = 0x8A; // ü
pub const CH_U_DIA_U: u8 = 0x8B; // Ü

unsafe fn begin_font_access() {
    // Sequencer: plane 2'ye, ardışık (sequential) erişim.
    outb(0x3C4, 0x00);
    outb(0x3C5, 0x01); // senkron reset
    outb(0x3C4, 0x02);
    outb(0x3C5, 0x04); // sadece plane 2'ye yaz
    outb(0x3C4, 0x04);
    outb(0x3C5, 0x07); // odd/even kapalı, ardışık
    outb(0x3C4, 0x00);
    outb(0x3C5, 0x03); // reset bitti
    // Graphics: plane 2'den oku, 0xA0000'e eşle.
    outb(0x3CE, 0x04);
    outb(0x3CF, 0x02); // plane 2'yi oku
    outb(0x3CE, 0x05);
    outb(0x3CF, 0x00); // grafik kipi normal
    outb(0x3CE, 0x06);
    outb(0x3CF, 0x00); // 0xA0000, metin eşlemesi
}

unsafe fn end_font_access() {
    // Sequencer: tekrar metin kipi (plane 0/1, odd/even).
    outb(0x3C4, 0x00);
    outb(0x3C5, 0x01);
    outb(0x3C4, 0x02);
    outb(0x3C5, 0x03);
    outb(0x3C4, 0x04);
    outb(0x3C5, 0x03);
    outb(0x3C4, 0x00);
    outb(0x3C5, 0x03);
    // Graphics: metin kipi, 0xB8000'e eşle.
    outb(0x3CE, 0x04);
    outb(0x3CF, 0x00);
    outb(0x3CE, 0x05);
    outb(0x3CF, 0x10); // odd/even açık
    outb(0x3CE, 0x06);
    outb(0x3CF, 0x0E); // 0xB8000
}

unsafe fn read_glyph(code: u8) -> [u8; 16] {
    let mut g = [0u8; 16];
    let base = FONT_MEM.add(code as usize * BYTES_PER_GLYPH);
    for (i, row) in g.iter_mut().enumerate() {
        *row = core::ptr::read_volatile(base.add(i));
    }
    g
}

unsafe fn write_glyph(code: u8, g: &[u8; 16]) {
    let base = FONT_MEM.add(code as usize * BYTES_PER_GLYPH);
    for (i, row) in g.iter().enumerate() {
        core::ptr::write_volatile(base.add(i), *row);
    }
}

// --- Aksan ekleme yardımcıları (8 piksel geniş satırlar; bit7 = en sol) ---

/// Harfin altına çengel (cedilla) ekler: ç, ş, Ç, Ş.
fn add_cedilla(g: &mut [u8; 16]) {
    g[14] |= 0b0001_1000;
    g[15] |= 0b0011_0000;
}

/// Harfin üstüne küçük yay (breve) ekler: ğ, Ğ.
fn add_breve(g: &mut [u8; 16]) {
    g[0] |= 0b0100_0010;
    g[1] |= 0b0100_0010;
    g[2] |= 0b0011_1100;
}

/// Harfin üstüne iki nokta (diaeresis) ekler. `top` küçük harfler için
/// noktaları biraz aşağıya alır (harfe yakın dursun).
fn add_diaeresis(g: &mut [u8; 16], lowercase: bool) {
    let (r0, r1) = if lowercase { (3, 4) } else { (0, 1) };
    g[r0] |= 0b0110_0110;
    g[r1] |= 0b0110_0110;
}

/// Türkçe harfleri üretip fonta yazar. Açılışta bir kez çağrılır.
pub fn install_turkish() {
    unsafe {
        begin_font_access();

        // Çengelli harfler
        let mut g = read_glyph(b'c');
        add_cedilla(&mut g);
        write_glyph(CH_C_CED_L, &g);
        let mut g = read_glyph(b'C');
        add_cedilla(&mut g);
        write_glyph(CH_C_CED_U, &g);
        let mut g = read_glyph(b's');
        add_cedilla(&mut g);
        write_glyph(CH_S_CED_L, &g);
        let mut g = read_glyph(b'S');
        add_cedilla(&mut g);
        write_glyph(CH_S_CED_U, &g);

        // Yaylı harfler
        let mut g = read_glyph(b'g');
        add_breve(&mut g);
        write_glyph(CH_G_BRV_L, &g);
        let mut g = read_glyph(b'G');
        add_breve(&mut g);
        write_glyph(CH_G_BRV_U, &g);

        // İki noktalı harfler
        let mut g = read_glyph(b'o');
        add_diaeresis(&mut g, true);
        write_glyph(CH_O_DIA_L, &g);
        let mut g = read_glyph(b'O');
        add_diaeresis(&mut g, false);
        write_glyph(CH_O_DIA_U, &g);
        let mut g = read_glyph(b'u');
        add_diaeresis(&mut g, true);
        write_glyph(CH_U_DIA_L, &g);
        let mut g = read_glyph(b'U');
        add_diaeresis(&mut g, false);
        write_glyph(CH_U_DIA_U, &g);

        // Noktasız ı: küçük 'i'den noktayı silerek.
        let mut g = read_glyph(b'i');
        for row in g.iter_mut().take(6) {
            *row = 0;
        }
        write_glyph(CH_I_DOTLESS, &g);

        // Noktalı büyük İ: büyük 'I'nın üstüne nokta ekleyerek.
        let mut g = read_glyph(b'I');
        g[1] |= 0b0001_1000;
        write_glyph(CH_I_DOTTED_U, &g);

        end_font_access();
    }
}
