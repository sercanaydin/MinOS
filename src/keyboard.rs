//! PS/2 klavye sürücüsü — Türkçe-Q düzeni (IRQ1 kesmesi + yedek yoklama).
//!
//! Türkçe-Q'da özel harflerin konumu (ABD klavyesindeki karşılığı):
//!   ğ→[   ü→]   ş→;   i/İ→'   ö→,   ç→.   ı→ QWERTY 'i' tuşu

use crate::port::inb;

const DATA: u16 = 0x60;

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

    // 102. tuş (Z'nin solu, "<>" tuşu) — programlama için kullanışlı.
    m[0x56] = if shift { '>' } else { '<' };

    m[0x39] = ' '; // Boşluk

    m
}

/// AltGr (Sağ Alt) basılıyken üretilen karakterler — Türkçe-Q programlama katmanı.
/// `{ } [ ] \ | @ # $ ~ \`` gibi C için gereken simgeler buradadır.
static MAP_ALTGR: [char; 128] = build_altgr();

const fn build_altgr() -> [char; 128] {
    let mut m = ['\0'; 128];
    m[0x04] = '#'; // 3
    m[0x05] = '$'; // 4
    m[0x08] = '{'; // 7
    m[0x09] = '['; // 8
    m[0x0A] = ']'; // 9
    m[0x0B] = '}'; // 0
    m[0x0C] = '\\'; // '*' tuşu
    m[0x0D] = '|'; // '-' tuşu
    m[0x10] = '@'; // q
    m[0x1B] = '~'; // 'ü' tuşu
    m[0x2B] = '`'; // ',' tuşu
    // '<' / '>' — bazı klavyelerde (örn. ANSI MacBook) Z'nin solundaki 102. tuş
    // (0x56) yoktur. Her klavyede ulaşılabilsin diye AltGr+ö = '<', AltGr+ç = '>'.
    m[0x33] = '<'; // 'ö' tuşu
    m[0x34] = '>'; // 'ç' tuşu
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

// Yön/düzenleme tuşları — Özel Kullanım Alanı kod noktaları (metin değildir).
// Editör bunları imleç hareketi için yorumlar; diğer tüketiciler yok sayar.
pub const KEY_LEFT: char = '\u{E010}';
pub const KEY_RIGHT: char = '\u{E011}';
pub const KEY_UP: char = '\u{E012}';
pub const KEY_DOWN: char = '\u{E013}';
pub const KEY_HOME: char = '\u{E014}';
pub const KEY_END: char = '\u{E015}';
pub const KEY_DEL: char = '\u{E016}';

static mut SHIFT_PRESSED: bool = false;
static mut CAPS_ON: bool = false;
static mut ALTGR_PRESSED: bool = false;
// 0xE0 genişletilmiş tuş ön eki görüldü mü (sonraki bayt için).
static mut EXTENDED: bool = false;

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

/// IRQ1: veri portundan tarama kodunu okuyup işler.
pub fn irq_feed() {
    let code = unsafe { inb(DATA) };
    feed(code);
}

/// 8042 yedek yoklama (klavye IRQ1 devre dışıysa elle çağrılabilir).
#[allow(dead_code)]
pub fn poll_port() {
    let status = unsafe { inb(0x64) };
    if status & 0x01 == 0 || status & 0x20 != 0 {
        return;
    }
    let code = unsafe { inb(DATA) };
    feed(code);
}

/// Tarama kodunu karaktere çevirip tampona koyar.
pub fn feed(code: u8) {
    // 0xE0: genişletilmiş tuş ön eki (Sağ Alt / AltGr, oklar, ...).
    if code == 0xE0 {
        unsafe { EXTENDED = true };
        return;
    }
    let ext = unsafe {
        let e = EXTENDED;
        EXTENDED = false;
        e
    };
    // Genişletilmiş tuşlar: Sağ Alt (AltGr) ve yön/düzenleme tuşları.
    // (Bırakma kodları 0x80'li gelir; yalnızca basma kodlarını işleriz.)
    if ext {
        match code {
            0x38 => unsafe { ALTGR_PRESSED = true },
            0xB8 => unsafe { ALTGR_PRESSED = false },
            0x4B => push(KEY_LEFT),
            0x4D => push(KEY_RIGHT),
            0x48 => push(KEY_UP),
            0x50 => push(KEY_DOWN),
            0x47 => push(KEY_HOME),
            0x4F => push(KEY_END),
            0x53 => push(KEY_DEL),
            _ => {}
        }
        return;
    }

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
    let altgr = unsafe { ALTGR_PRESSED };
    let mut ch = if altgr {
        MAP_ALTGR[code as usize]
    } else if shift {
        MAP_SHIFT[code as usize]
    } else {
        MAP[code as usize]
    };

    if !altgr && caps && ch.is_alphabetic() {
        ch = if shift { to_lower_tr(ch) } else { to_upper_tr(ch) };
    }

    if ch != '\0' {
        push(ch);
    }
}

/// Bir karakter üretilene kadar bekler. IRQ1 (IF=1 iken) tampona iter; ayrıca
/// 8042'yi doğrudan yoklarız ki `int 0x80` (IF=0, kesme kapısı) bağlamından
/// çağrıldığında da çalışsın. `hlt` KULLANMAYIZ: IF=0 iken sonsuza dek kilitler.
pub fn read_char() -> char {
    loop {
        if let Some(c) = pop() {
            return c;
        }
        crate::input::poll();
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
