//! PS/2 klavye sürücüsü — Türkçe-Q düzeni (kesmesiz, yoklama / polling ile).
//!
//! Klavye denetleyicisinin durum portunu (0x64) yoklayıp veri portundan (0x60)
//! Set 1 tarama kodlarını okuruz. Tarama kodları tuşun FİZİKSEL konumuna
//! karşılık gelir; biz de bunları Türkçe-Q düzenine göre karakterlere çeviririz.
//!
//! Türkçe-Q'da özel harflerin konumu (ABD klavyesindeki karşılığı):
//!   ğ→[   ü→]   ş→;   i/İ→'   ö→,   ç→.   ı→ QWERTY 'i' tuşu

/// Tarama kodu → karakter (Shift basılı değilken). 0 = eşlenmemiş.
static MAP: [char; 128] = build_map(false);
/// Tarama kodu → karakter (Shift basılıyken).
static MAP_SHIFT: [char; 128] = build_map(true);

const fn build_map(shift: bool) -> [char; 128] {
    let mut m = ['\0'; 128];

    // Üst sıra: rakamlar ve Türkçe-Q simgeleri.
    m[0x02] = if shift { '!' } else { '1' };
    m[0x03] = if shift { '\'' } else { '2' };
    m[0x04] = if shift { '^' } else { '3' };
    m[0x05] = if shift { '+' } else { '4' };
    m[0x06] = if shift { '%' } else { '5' };
    m[0x07] = if shift { '&' } else { '6' };
    m[0x08] = if shift { '/' } else { '7' };
    m[0x09] = if shift { '(' } else { '8' };
    m[0x0A] = if shift { ')' } else { '9' };
    m[0x0B] = if shift { '=' } else { '0' };
    m[0x0C] = if shift { '?' } else { '*' };
    m[0x0D] = if shift { '_' } else { '-' };
    m[0x0E] = '\u{8}'; // Backspace
    m[0x0F] = '\t';

    // İlk harf sırası: q w e r t y u ı o p ğ ü
    m[0x10] = if shift { 'Q' } else { 'q' };
    m[0x11] = if shift { 'W' } else { 'w' };
    m[0x12] = if shift { 'E' } else { 'e' };
    m[0x13] = if shift { 'R' } else { 'r' };
    m[0x14] = if shift { 'T' } else { 't' };
    m[0x15] = if shift { 'Y' } else { 'y' };
    m[0x16] = if shift { 'U' } else { 'u' };
    m[0x17] = if shift { 'I' } else { 'ı' }; // QWERTY 'i' → noktasız ı / I
    m[0x18] = if shift { 'O' } else { 'o' };
    m[0x19] = if shift { 'P' } else { 'p' };
    m[0x1A] = if shift { 'Ğ' } else { 'ğ' };
    m[0x1B] = if shift { 'Ü' } else { 'ü' };
    m[0x1C] = '\n'; // Enter

    // İkinci harf sırası: a s d f g h j k l ş i
    m[0x1E] = if shift { 'A' } else { 'a' };
    m[0x1F] = if shift { 'S' } else { 's' };
    m[0x20] = if shift { 'D' } else { 'd' };
    m[0x21] = if shift { 'F' } else { 'f' };
    m[0x22] = if shift { 'G' } else { 'g' };
    m[0x23] = if shift { 'H' } else { 'h' };
    m[0x24] = if shift { 'J' } else { 'j' };
    m[0x25] = if shift { 'K' } else { 'k' };
    m[0x26] = if shift { 'L' } else { 'l' };
    m[0x27] = if shift { 'Ş' } else { 'ş' };
    m[0x28] = if shift { 'İ' } else { 'i' }; // ' tuşu → noktalı i / İ
    m[0x29] = if shift { 'é' } else { '"' };

    // Üçüncü harf sırası: z x c v b n m ö ç .
    m[0x2B] = if shift { ';' } else { ',' };
    m[0x2C] = if shift { 'Z' } else { 'z' };
    m[0x2D] = if shift { 'X' } else { 'x' };
    m[0x2E] = if shift { 'C' } else { 'c' };
    m[0x2F] = if shift { 'V' } else { 'v' };
    m[0x30] = if shift { 'B' } else { 'b' };
    m[0x31] = if shift { 'N' } else { 'n' };
    m[0x32] = if shift { 'M' } else { 'm' };
    m[0x33] = if shift { 'Ö' } else { 'ö' };
    m[0x34] = if shift { 'Ç' } else { 'ç' };
    m[0x35] = if shift { ':' } else { '.' };

    m[0x39] = ' '; // Boşluk

    m
}

const LSHIFT_DOWN: u8 = 0x2A;
const RSHIFT_DOWN: u8 = 0x36;
const LSHIFT_UP: u8 = 0xAA;
const RSHIFT_UP: u8 = 0xB6;
const CAPS_LOCK: u8 = 0x3A;
const ESC: u8 = 0x01;
const F1: u8 = 0x3B;

/// Terminal ↔ masaüstü geçişi için özel tuş (F1). Özel kullanım alanı kod
/// noktası; normal metin olarak yorumlanmaz.
pub const KEY_TOGGLE: char = '\u{E001}';
/// Esc tuşu (masaüstünden terminale dönüş için de kullanılır).
pub const KEY_ESC: char = '\u{1B}';

static mut SHIFT_PRESSED: bool = false;
static mut CAPS_ON: bool = false;

// Çözümlenmiş karakterler için küçük halka tampon. Giriş tek noktadan
// (input::poll) işlendiği için klavye ve fare baytları karışmaz.
const RING: usize = 32;
static mut BUF: [char; RING] = ['\0'; RING];
static mut HEAD: usize = 0;
static mut TAIL: usize = 0;

fn push(c: char) {
    unsafe {
        let next = (HEAD + 1) % RING;
        if next != TAIL {
            BUF[HEAD] = c;
            HEAD = next;
        }
    }
}

/// Tampondaki bir sonraki karakteri alır (yoksa None).
pub fn pop() -> Option<char> {
    unsafe {
        if HEAD == TAIL {
            None
        } else {
            let c = BUF[TAIL];
            TAIL = (TAIL + 1) % RING;
            Some(c)
        }
    }
}

/// 8042'den okunan bir klavye tarama kodunu işler; ürettiği karakteri
/// (varsa) tampona koyar. `input::poll` tarafından çağrılır.
pub fn feed(code: u8) {
    match code {
        LSHIFT_DOWN | RSHIFT_DOWN => {
            unsafe { SHIFT_PRESSED = true };
            return;
        }
        LSHIFT_UP | RSHIFT_UP => {
            unsafe { SHIFT_PRESSED = false };
            return;
        }
        CAPS_LOCK => {
            unsafe { CAPS_ON = !CAPS_ON };
            return;
        }
        ESC => {
            push(KEY_ESC);
            return;
        }
        F1 => {
            push(KEY_TOGGLE);
            return;
        }
        _ => {}
    }

    // Tuş bırakma (key-up) kodlarını yok say.
    if code & 0x80 != 0 || code as usize >= MAP.len() {
        return;
    }

    let shift = unsafe { SHIFT_PRESSED };
    let caps = unsafe { CAPS_ON };
    let mut ch = if shift { MAP_SHIFT[code as usize] } else { MAP[code as usize] };

    if caps && ch.is_alphabetic() {
        ch = if shift { to_lower_tr(ch) } else { to_upper_tr(ch) };
    }

    if ch != '\0' {
        push(ch);
    }
}

/// Bir karakter üretilene kadar bekler ve onu döndürür (kabuk için).
pub fn read_char() -> char {
    loop {
        crate::input::poll();
        if let Some(c) = pop() {
            return c;
        }
        core::hint::spin_loop();
    }
}

fn to_upper_tr(c: char) -> char {
    match c {
        'i' => 'İ',
        'ı' => 'I',
        'ç' => 'Ç',
        'ş' => 'Ş',
        'ğ' => 'Ğ',
        'ö' => 'Ö',
        'ü' => 'Ü',
        'a'..='z' => ((c as u8) - 32) as char,
        _ => c,
    }
}

fn to_lower_tr(c: char) -> char {
    match c {
        'İ' => 'i',
        'I' => 'ı',
        'Ç' => 'ç',
        'Ş' => 'ş',
        'Ğ' => 'ğ',
        'Ö' => 'ö',
        'Ü' => 'ü',
        'A'..='Z' => ((c as u8) + 32) as char,
        _ => c,
    }
}
