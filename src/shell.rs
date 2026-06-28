//! Çok basit, etkileşimli bir kabuk (shell).
//!
//! Klavyeden bir satır okur, komutu ayrıştırır ve çalıştırır. Bellek ayırıcı
//! (heap) olmadığı için sabit boyutlu bir karakter tamponu kullanırız. Tampon
//! `char` tutar; böylece Türkçe harfler de saklanıp yansıtılabilir.

use crate::vga::Color;
use crate::{fs, keyboard, print, println, vga};
use alloc::string::{String, ToString};

const PROMPT: &str = "os";
const BUF_LEN: usize = 128;

// Geçerli çalışma dizini (entry indeksi; fs::ROOT = kök).
use core::sync::atomic::{AtomicU8, Ordering};
static CWD: AtomicU8 = AtomicU8::new(fs::ROOT);

fn cwd() -> u8 {
    CWD.load(Ordering::Relaxed)
}
/// Sistem çağrıları (fd) için: kabuğun geçerli çalışma dizini.
pub fn current_dir() -> u8 {
    CWD.load(Ordering::Relaxed)
}
fn set_cwd(v: u8) {
    CWD.store(v, Ordering::Relaxed);
}

// Açılışta biçimlendirme için kullanılan disk boyutu (blok = 512 bayt).
pub const DISK_BLOCKS: u32 = 4096; // 2 MiB

// Dosya içeriğini oku/derle için tampon (heap olmadığından sabit boyutlu).
static mut FILE_BUF: [u8; fs::MAX_FILE_SIZE] = [0; fs::MAX_FILE_SIZE];

// `run <prog> <arg...>` ile verilen argüman dizesi (UTF-8). SYS_GETARG bunu
// kullanıcı alanına kopyalar; crt0 argv'ye böler.
const RUN_ARG_CAP: usize = 256;
static mut RUN_ARG: [u8; RUN_ARG_CAP] = [0; RUN_ARG_CAP];
static mut RUN_ARG_LEN: usize = 0;

/// `run` argüman dizesini ayarlar (program adından sonraki kısım).
fn set_run_arg(args: &[char]) {
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(RUN_ARG) };
    let mut n = 0usize;
    for &c in args {
        let mut tmp = [0u8; 4];
        let s = c.encode_utf8(&mut tmp);
        let b = s.as_bytes();
        if n + b.len() > RUN_ARG_CAP {
            break;
        }
        buf[n..n + b.len()].copy_from_slice(b);
        n += b.len();
    }
    unsafe { RUN_ARG_LEN = n };
}

/// SYS_GETARG: saklı argüman dizesini `dst`'e kopyalar, bayt sayısını döner.
pub fn run_arg_copy(dst: &mut [u8]) -> u32 {
    let src = unsafe { &*core::ptr::addr_of!(RUN_ARG) };
    let len = unsafe { RUN_ARG_LEN };
    let n = core::cmp::min(len, dst.len());
    dst[..n].copy_from_slice(&src[..n]);
    n as u32
}

pub fn run() -> ! {
    let mut buffer = ['\0'; BUF_LEN];

    loop {
        prompt();

        let len = read_line(&mut buffer);
        let line = &buffer[..len];
        let (start, end) = trim(line);
        let line = &line[start..end];

        if !line.is_empty() {
            execute(line);
        }
    }
}

/// Enter'a basılana kadar bir satır okur, yazılanları ekrana yansıtır ve
/// Backspace'i destekler. Doldurulan karakter sayısını döndürür.
fn read_line(buffer: &mut [char; BUF_LEN]) -> usize {
    let mut len = 0;

    loop {
        let c = keyboard::read_char();
        match c {
            '\n' => {
                print!("\n");
                return len;
            }
            '\u{8}' => {
                if len > 0 {
                    len -= 1;
                    print!("\u{8}");
                }
            }
            keyboard::KEY_TOGGLE => {
                // Metin modundan yeni geçtiysek ekran temizlendi; bu satırı
                // (istem + yazılanlar) yeniden basıp düzenlemeye devam et.
                if enter_gui() {
                    prompt();
                    for &c in buffer.iter().take(len) {
                        print!("{c}");
                    }
                }
            }
            // Yön/düzenleme tuşları (PUA E010+) terminalde anlamsız — yok say.
            c if (c as u32) >= 0xE000 => {}
            c if !c.is_control() => {
                if len < BUF_LEN {
                    buffer[len] = c;
                    len += 1;
                    print!("{c}");
                }
            }
            _ => {}
        }
    }
}

/// Baştaki/sondaki boşlukları atlayarak [başlangıç, bitiş) aralığı döndürür.
fn trim(s: &[char]) -> (usize, usize) {
    let mut start = 0;
    let mut end = s.len();
    while start < end && s[start] == ' ' {
        start += 1;
    }
    while end > start && s[end - 1] == ' ' {
        end -= 1;
    }
    (start, end)
}

/// Bir karakter dilimini bir ASCII komut adıyla karşılaştırır.
fn eq(cmd: &[char], name: &str) -> bool {
    // Karakter bazlı karşılaştırma. `name.len()` baytları sayar; çok baytlı
    // UTF-8 karakterler (ı, ü, ç...) içeren komut adlarında bayt/karakter
    // karışmasın diye karakterleri tek tek eşleştiriyoruz.
    let mut chars = name.chars();
    for &c in cmd {
        match chars.next() {
            Some(n) if n == c => {}
            _ => return false,
        }
    }
    chars.next().is_none()
}

fn execute(line: &[char]) {
    // Komutu ilk boşluktan ayır: "komut" + "argümanlar".
    let split = line.iter().position(|&c| c == ' ');
    let (cmd, args) = match split {
        Some(i) => {
            let (s, e) = trim(&line[i + 1..]);
            (&line[..i], &line[i + 1..][s..e])
        }
        None => (line, &line[0..0]),
    };

    if eq(cmd, "help") {
        help();
    } else if eq(cmd, "echo") {
        for &c in args {
            print!("{c}");
        }
        println!();
    } else if eq(cmd, "clear") {
        vga::clear();
    } else if eq(cmd, "about") {
        about();
    } else if eq(cmd, "colors") {
        colors_demo();
    } else if eq(cmd, "turkce") {
        turkce_demo();
    } else if eq(cmd, "ls") {
        fs_ls();
    } else if eq(cmd, "cat") {
        fs_cat(args);
    } else if eq(cmd, "write") || eq(cmd, "yaz") {
        fs_write(args, false);
    } else if eq(cmd, "append") || eq(cmd, "ekle") {
        fs_write(args, true);
    } else if eq(cmd, "rm") || eq(cmd, "rmdir") {
        fs_rm(args);
    } else if eq(cmd, "mkdir") || eq(cmd, "dizin") {
        fs_mkdir(args);
    } else if eq(cmd, "cd") {
        fs_cd(args);
    } else if eq(cmd, "pwd") {
        fs_pwd();
    } else if eq(cmd, "format") {
        fs_format();
    } else if eq(cmd, "nvmeformat") || eq(cmd, "nvme-biçimlendir") || eq(cmd, "nvme-bicimlendir")
    {
        nvme_format(args);
    } else if eq(cmd, "depo") || eq(cmd, "donanim") || eq(cmd, "hw") {
        dev_info();
    } else if eq(cmd, "mem") || eq(cmd, "bellek") {
        mem_info();
    } else if eq(cmd, "inttest") || eq(cmd, "kesme") {
        int_test();
    } else if eq(cmd, "uptime") || eq(cmd, "süre") || eq(cmd, "sure") {
        uptime_info();
    } else if eq(cmd, "gorevtest") || eq(cmd, "görev") || eq(cmd, "gorev") || eq(cmd, "tasks") {
        task_test();
    } else if eq(cmd, "preempt") || eq(cmd, "önleyici") || eq(cmd, "onleyici") {
        preempt_test();
    } else if eq(cmd, "sleeptest") || eq(cmd, "uyku") {
        sleep_test();
    } else if eq(cmd, "userpreempt") || eq(cmd, "r3preempt") || eq(cmd, "sureconleyici") {
        user_preempt_test();
    } else if eq(cmd, "cprog") || eq(cmd, "cdemo") || eq(cmd, "chello") {
        crate::user::run_c_demo();
    } else if eq(cmd, "apps") || eq(cmd, "uygulamalar") {
        install_c_apps();
    } else if eq(cmd, "ring3") || eq(cmd, "r3") || eq(cmd, "kullanici") || eq(cmd, "kullanıcı") {
        crate::user::run_hello();
    } else if eq(cmd, "korumatest") || eq(cmd, "koruma") {
        crate::user::run_fault();
    } else if eq(cmd, "run") || eq(cmd, "calistir") || eq(cmd, "çalıştır") {
        run_elf_file(args);
    } else if eq(cmd, "kur") || eq(cmd, "elfkur") {
        elf_install(args);
    } else if eq(cmd, "saat") || eq(cmd, "tarih") {
        show_clock();
    } else if eq(cmd, "web") || eq(cmd, "getir") {
        web_cmd(args);
    } else if eq(cmd, "df") {
        fs_df();
    } else if eq(cmd, "gui") || eq(cmd, "guı") || eq(cmd, "masaüstü") {
        enter_gui();
    } else if eq(cmd, "reboot") {
        reboot();
    } else {
        vga::set_color(Color::LightRed, Color::Black);
        print!("bilinmeyen komut: '");
        for &c in cmd {
            print!("{c}");
        }
        println!("' (yardım için 'help' yazın)");
        vga::set_color(Color::LightGray, Color::Black);
    }
}

fn help() {
    println!("Genel komutlar:");
    println!("  help          - bu yardım metnini gösterir");
    println!("  echo <metin>  - verilen metni ekrana yazar");
    println!("  clear         - ekranı temizler");
    println!("  about         - sistem hakkında bilgi");
    println!("  saat          - tarih ve saati gösterir");
    println!("  colors        - renk paletini gösterir");
    println!("  turkce        - Türkçe karakterleri gösterir");
    println!("  gui           - grafik masaüstüne geçer (F1 ile de açılır)");
    println!("  web <adres>   - sayfa çeker (http + https/TLS), örn: web https://...");
    println!("  reboot        - bilgisayarı yeniden başlatır");
    println!("Dosya sistemi (kalıcı disk):");
    println!("  ls                  - bulunulan dizini listeler");
    println!("  cat <ad>            - dosya içeriğini gösterir");
    println!("  yaz <ad> <metin>    - dosyaya yazar (write)");
    println!("  ekle <ad> <metin>   - dosyanın sonuna ekler (append)");
    println!("  rm <ad>             - dosyayı/boş dizini siler");
    println!("  mkdir <ad>          - yeni dizin oluşturur");
    println!("  cd <ad> | .. | /    - dizin değiştirir");
    println!("  pwd                 - bulunulan dizin yolunu gösterir");
    println!("  df                  - disk kullanımını gösterir");
    println!("  depo                - depolama donanımını gösterir (NVMe/ATA)");
    println!("  mem                 - bellek/sayfalama durumunu gösterir");
    println!("  inttest             - kesme tablosunu (IDT) sınar — int3 yakala/dön");
    println!("  uptime (veya süre)  - donanım zamanlayıcısı (PIT/IRQ0) çalışma süresi");
    println!("  gorevtest (görev)   - işbirlikçi çok görevlilik (bağlam değiştirme) sınaması");
    println!("  preempt (önleyici)  - önleyici çok görevlilik (timer ile zorla geçiş) sınaması");
    println!("  sleeptest (uyku)    - görev uyku (sleep) sınaması");
    println!("  userpreempt (r3preempt) - ring3 önleyici çok süreç (ayrı sayfa tabloları)");
    println!("  cprog (chello)      - clang ile derlenmiş C programını ring 3'te çalıştırır");
    println!("  apps                - C uygulamalarını diske kurar (web, calc, boya, c4)");
    println!("  ring3 (veya r3)     - gömülü ELF'i yükler, ring 3'te çalıştırır");
    println!("  kur [ad]            - gömülü ELF'i diske yazar (varsayılan: prog)");
    println!("  run <dosya> [arg]   - diskteki bir ELF'i yükleyip ring 3'te çalıştırır");
    println!("  run c4 <dosya.c>    - OS içinde C kaynağını derler ve çalıştırır (c4)");
    println!("  korumatest          - ring 3'ten çekirdeğe yazmayı dener (koruma/panik)");
    println!("  format              - aktif diski sıfırlar (TÜM veri silinir)");
    println!("  nvmeformat EVET     - NVMe diskini kalıcı yapar (açık onay ister)");
}

// --- İnternet (text tarayıcı) ---

/// `web <adres>` — verilen adresi HTTP ile çekip metnini ekrana basar.
fn web_cmd(args: &[char]) {
    let (s, e) = trim(args);
    let a = &args[s..e];
    if a.is_empty() {
        println!("kullanım: web <adres>   (örn: web example.com  ya da  web https://example.com)");
        return;
    }

    // İlk kelimeyi al (boşluğa kadar) ve String'e çevir.
    let mut raw = String::new();
    for &c in a {
        if c == ' ' {
            break;
        }
        raw.push(c);
    }

    // Şemayı belirle: https varsayılan değil; açıkça http:// yoksa ve https://
    // yazılmışsa TLS. Şema yoksa düz HTTP dene.
    let mut secure = raw.starts_with("https://");
    let url = raw.trim_start_matches("http://").trim_start_matches("https://");
    let (mut host, mut path) = match url.find('/') {
        Some(i) => (url[..i].to_string(), url[i..].to_string()),
        None => (url.to_string(), String::from("/")),
    };

    // En çok 5 yönlendirme (301/302/...) takip et.
    for _ in 0..5 {
        vga::set_color(Color::LightCyan, Color::Black);
        let scheme = if secure { "https" } else { "http" };
        println!("[web] {scheme}://{host}{path} bağlanılıyor...");
        vga::set_color(Color::LightGray, Color::Black);

        let result = if secure {
            crate::net::fetch_https(&host, &path)
        } else {
            crate::net::fetch(&host, &path)
        };
        let resp = match result {
            Ok(r) => r,
            Err(err) => {
                vga::set_color(Color::LightRed, Color::Black);
                println!("[web] hata: {err}");
                if secure {
                    vga::set_color(Color::DarkGray, Color::Black);
                    println!("      Not: TLS 1.3 (rustls), kök sertifikalar doğrulanır.");
                    println!("      Sertifika hatasında saat/tarihin doğru olduğundan emin olun.");
                }
                vga::set_color(Color::LightGray, Color::Black);
                return;
            }
        };

        let code = status_code(&resp);
        if (300..400).contains(&code) {
            if let Some(loc) = header_value(&resp, "location") {
                if let Some(rest) = loc.strip_prefix("http://") {
                    secure = false;
                    match rest.find('/') {
                        Some(i) => {
                            host = rest[..i].to_string();
                            path = rest[i..].to_string();
                        }
                        None => {
                            host = rest.to_string();
                            path = String::from("/");
                        }
                    }
                    vga::set_color(Color::DarkGray, Color::Black);
                    println!("[web] yönlendiriliyor -> {host}{path}");
                    vga::set_color(Color::LightGray, Color::Black);
                    continue;
                } else if let Some(rest) = loc.strip_prefix("https://") {
                    secure = true;
                    match rest.find('/') {
                        Some(i) => {
                            host = rest[..i].to_string();
                            path = rest[i..].to_string();
                        }
                        None => {
                            host = rest.to_string();
                            path = String::from("/");
                        }
                    }
                    vga::set_color(Color::DarkGray, Color::Black);
                    println!("[web] yönlendiriliyor (TLS) -> {host}{path}");
                    vga::set_color(Color::LightGray, Color::Black);
                    continue;
                } else if let Some(rest) = loc.strip_prefix('/') {
                    path = String::from("/");
                    path.push_str(rest);
                    vga::set_color(Color::DarkGray, Color::Black);
                    println!("[web] yönlendiriliyor -> {host}{path}");
                    vga::set_color(Color::LightGray, Color::Black);
                    continue;
                }
            }
        }

        render_html(&resp);
        return;
    }

    vga::set_color(Color::LightRed, Color::Black);
    println!("[web] çok fazla yönlendirme; durduruldu.");
    vga::set_color(Color::LightGray, Color::Black);
}

/// Yanıtın ilk satırından HTTP durum kodunu ayıklar ("HTTP/1.1 301 ..." -> 301).
fn status_code(resp: &str) -> u16 {
    resp.lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .unwrap_or(0)
}

/// Başlıklar arasında verilen ismi (büyük/küçük harf duyarsız) arar, değerini döndürür.
fn header_value(resp: &str, name: &str) -> Option<String> {
    for line in resp.lines() {
        if line.is_empty() {
            break; // başlıklar bitti
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

/// HTTP yanıtını basitçe metne çevirip (HTML etiketlerini atarak) gösterir.
fn render_html(resp: &str) {
    if let Some(line) = resp.lines().next() {
        vga::set_color(Color::LightGreen, Color::Black);
        println!("{line}");
        vga::set_color(Color::LightGray, Color::Black);
    }
    let body = match resp.find("\r\n\r\n") {
        Some(i) => &resp[i + 4..],
        None => resp,
    };

    let mut out = String::new();
    let mut in_tag = false;
    let mut last_space = true;
    for ch in body.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                if !last_space {
                    out.push(' ');
                    last_space = true;
                }
            }
            _ if in_tag => {}
            c if c.is_whitespace() => {
                if !last_space {
                    out.push(' ');
                    last_space = true;
                }
            }
            c => {
                out.push(c);
                last_space = false;
            }
        }
    }

    let limit = 2000;
    for (i, ch) in out.chars().enumerate() {
        if i >= limit {
            println!("\n... (kısaltıldı)");
            break;
        }
        print!("{ch}");
    }
    println!();
}

/// Tam tarih ve saati gösterir.
fn show_clock() {
    let dt = crate::rtc::now();
    let gun = weekday_tr(crate::rtc::weekday(&dt));
    vga::set_color(Color::LightCyan, Color::Black);
    println!(
        "{:02}.{:02}.{} {:02}:{:02}:{:02}  {}",
        dt.day, dt.month, dt.year, dt.hour, dt.min, dt.sec, gun
    );
    vga::set_color(Color::LightGray, Color::Black);
}

fn weekday_tr(w: u8) -> &'static str {
    match w {
        0 => "Pazar",
        1 => "Pazartesi",
        2 => "Salı",
        3 => "Çarşamba",
        4 => "Perşembe",
        5 => "Cuma",
        6 => "Cumartesi",
        _ => "",
    }
}

fn about() {
    vga::set_color(Color::LightCyan, Color::Black);
    println!("MinOS - giriş seviyesi işletim sistemi");
    vga::set_color(Color::LightGray, Color::Black);
    println!("  Dil       : Rust + Assembly (no_std)");
    println!("  Mimari    : x86 (32-bit), Multiboot v1");
    println!("  Önyükleme : GRUB uyumlu / QEMU -kernel");
    println!("  Klavye    : Türkçe-Q düzeni");
}

fn colors_demo() {
    let colors = [
        (Color::Red, "kırmızı"),
        (Color::Green, "yeşil"),
        (Color::Blue, "mavi"),
        (Color::Yellow, "sarı"),
        (Color::Magenta, "magenta"),
        (Color::Cyan, "camgöbeği"),
        (Color::White, "beyaz"),
    ];
    for (color, name) in colors {
        vga::set_color(color, Color::Black);
        print!("{name} ");
    }
    vga::set_color(Color::LightGray, Color::Black);
    println!();
}

fn turkce_demo() {
    vga::set_color(Color::LightCyan, Color::Black);
    println!("Türkçe karakterler:");
    vga::set_color(Color::LightGray, Color::Black);
    println!("  küçük : ç ğ ı i ö ş ü");
    println!("  BÜYÜK : Ç Ğ I İ Ö Ş Ü");
    println!("  Pijamalı hasta yağız şoföre çabucak güvendi.");
}

// --- Dosya sistemi komutları ---

/// Argümanları ilk kelime (ad) ve geri kalan (metin) olarak ayırır.
fn split_arg(args: &[char]) -> (&[char], &[char]) {
    match args.iter().position(|&c| c == ' ') {
        Some(i) => {
            let (s, e) = trim(&args[i + 1..]);
            (&args[..i], &args[i + 1..][s..e])
        }
        None => (args, &args[0..0]),
    }
}

/// Bir karakter dilimini UTF-8 dosya adı baytlarına çevirir (Türkçe harfler
/// de geçerlidir). Boşluk/kontrol karakteri veya çok uzunsa None döner.
fn name_to_bytes(src: &[char], dst: &mut [u8; 28]) -> Option<usize> {
    if src.is_empty() {
        return None;
    }
    let mut n = 0;
    for &c in src {
        if c == ' ' || c.is_control() {
            return None;
        }
        let mut tmp = [0u8; 4];
        let bytes = c.encode_utf8(&mut tmp).as_bytes();
        if n + bytes.len() > dst.len() {
            return None;
        }
        dst[n..n + bytes.len()].copy_from_slice(bytes);
        n += bytes.len();
    }
    Some(n)
}

/// Karakterleri UTF-8 bayt dizisine kodlar; sonuna satır sonu ekler.
fn encode_text(text: &[char], buf: &mut [u8]) -> usize {
    let mut n = 0;
    for &c in text {
        let mut tmp = [0u8; 4];
        let bytes = c.encode_utf8(&mut tmp).as_bytes();
        if n + bytes.len() >= buf.len() {
            break;
        }
        buf[n..n + bytes.len()].copy_from_slice(bytes);
        n += bytes.len();
    }
    if n < buf.len() {
        buf[n] = b'\n';
        n += 1;
    }
    n
}

fn print_err(e: fs::FsError) {
    vga::set_color(Color::LightRed, Color::Black);
    println!("hata: {}", e.message());
    vga::set_color(Color::LightGray, Color::Black);
}

fn print_name(name: &[u8]) {
    let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    match core::str::from_utf8(&name[..len]) {
        Ok(s) => print!("{s}"),
        Err(_) => print!("?"),
    }
}

fn fs_ls() {
    let here = cwd();
    let mut count = 0;
    for i in 0..fs::MAX_FILES {
        match fs::entry_info(i) {
            Ok(Some(info)) => {
                if info.parent != here {
                    continue;
                }
                if info.is_dir() {
                    vga::set_color(Color::LightBlue, Color::Black);
                    print_name(&info.name);
                    print!("/");
                    vga::set_color(Color::LightGray, Color::Black);
                    println!("   <dizin>");
                } else {
                    vga::set_color(Color::LightCyan, Color::Black);
                    print_name(&info.name);
                    vga::set_color(Color::LightGray, Color::Black);
                    println!("  ({} bayt)", info.size);
                }
                count += 1;
            }
            Ok(None) => {}
            Err(e) => {
                print_err(e);
                return;
            }
        }
    }
    if count == 0 {
        println!("(dizin boş)");
    }
}

/// Geçerli dizinde yeni bir alt dizin oluşturur.
fn fs_mkdir(args: &[char]) {
    let (name, _) = split_arg(args);
    let mut nm = [0u8; 28];
    let nl = match name_to_bytes(name, &mut nm) {
        Some(l) => l,
        None => {
            println!("kullanım: mkdir <dizin-adı>");
            return;
        }
    };
    match fs::mkdir(&nm[..nl], cwd()) {
        Ok(()) => println!("dizin oluşturuldu"),
        Err(e) => print_err(e),
    }
}

/// Çalışma dizinini değiştirir. `cd ..` üst dizine, `cd /` köke gider.
fn fs_cd(args: &[char]) {
    let (name, _) = split_arg(args);
    // cd / veya argümansız cd -> kök
    if name.is_empty() || (name.len() == 1 && name[0] == '/') {
        set_cwd(fs::ROOT);
        return;
    }
    // cd ..
    if name.len() == 2 && name[0] == '.' && name[1] == '.' {
        let here = cwd();
        if here != fs::ROOT {
            set_cwd(fs::parent_of(here).unwrap_or(fs::ROOT));
        }
        return;
    }
    let mut nm = [0u8; 28];
    let nl = match name_to_bytes(name, &mut nm) {
        Some(l) => l,
        None => {
            print_err(fs::FsError::NotFound);
            return;
        }
    };
    // Geçerli dizindeki girdiyi bul ve dizinse içine gir.
    let here = cwd();
    for i in 0..fs::MAX_FILES {
        if let Ok(Some(info)) = fs::entry_info(i) {
            if info.parent == here && name_matches(&info.name, &nm[..nl]) {
                if info.is_dir() {
                    set_cwd(i as u8);
                } else {
                    print_err(fs::FsError::NotDir);
                }
                return;
            }
        }
    }
    print_err(fs::FsError::NotFound);
}

/// Geçerli dizin yolunu yazar.
fn fs_pwd() {
    let dir = cwd();
    if dir == fs::ROOT {
        println!("/");
        return;
    }
    let mut chain = [0u8; fs::MAX_FILES];
    let mut n = 0;
    let mut cur = dir;
    while cur != fs::ROOT && n < chain.len() {
        chain[n] = cur;
        n += 1;
        cur = fs::parent_of(cur).unwrap_or(fs::ROOT);
    }
    for i in (0..n).rev() {
        print!("/");
        if let Ok(Some(info)) = fs::entry_info(chain[i] as usize) {
            print_name(&info.name);
        }
    }
    println!();
}

/// Saklanan ad (null ile dolu) ile sorgu baytlarını karşılaştırır.
fn name_matches(stored: &[u8], query: &[u8]) -> bool {
    let len = stored.iter().position(|&b| b == 0).unwrap_or(stored.len());
    &stored[..len] == query
}

fn fs_cat(args: &[char]) {
    let (name, _) = split_arg(args);
    let mut nm = [0u8; 28];
    let nl = match name_to_bytes(name, &mut nm) {
        Some(l) => l,
        None => {
            print_err(fs::FsError::NameTooLong);
            return;
        }
    };
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(FILE_BUF) };
    match fs::read_file(&nm[..nl], cwd(), buf) {
        Ok(size) => match core::str::from_utf8(&buf[..size]) {
            Ok(s) => print!("{s}"),
            Err(_) => println!("(ikili veri, {size} bayt)"),
        },
        Err(e) => print_err(e),
    }
}

fn fs_write(args: &[char], append: bool) {
    let (name, text) = split_arg(args);
    let mut nm = [0u8; 28];
    let nl = match name_to_bytes(name, &mut nm) {
        Some(l) => l,
        None => {
            println!("kullanım: yaz <ad> <metin>");
            return;
        }
    };
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(FILE_BUF) };
    let len = encode_text(text, buf);
    let res = if append {
        fs::append_file(&nm[..nl], cwd(), &buf[..len])
    } else {
        fs::write_file(&nm[..nl], cwd(), &buf[..len])
    };
    match res {
        Ok(()) => println!("tamam ({len} bayt)"),
        Err(e) => print_err(e),
    }
}

fn fs_rm(args: &[char]) {
    let (name, _) = split_arg(args);
    let mut nm = [0u8; 28];
    let nl = match name_to_bytes(name, &mut nm) {
        Some(l) => l,
        None => {
            print_err(fs::FsError::NotFound);
            return;
        }
    };
    match fs::delete(&nm[..nl], cwd()) {
        Ok(()) => println!("silindi"),
        Err(e) => print_err(e),
    }
}

fn fs_format() {
    match fs::format(DISK_BLOCKS) {
        Ok(()) => {
            set_cwd(fs::ROOT);
            println!("disk biçimlendirildi ({DISK_BLOCKS} blok).");
        }
        Err(e) => print_err(e),
    }
}

/// Algılanan depolama donanımını ve geçerli arka ucu özetler. Gerçek donanımda
/// (USB'den açıp) bu komutu çalıştırınca kalıcı depolamanın mümkün olup
/// olmadığını öğreniriz.
/// `run <dosya>`: diskteki bir dosyayı okuyup ELF olarak ring 3'te çalıştırır.
fn run_elf_file(args: &[char]) {
    let (name, rest) = split_arg(args);
    let mut nm = [0u8; 28];
    let nl = match name_to_bytes(name, &mut nm) {
        Some(l) => l,
        None => {
            println!("kullanım: run <dosya> [argümanlar]");
            return;
        }
    };
    // Programa iletilecek argüman dizesini (ad sonrası) sakla.
    set_run_arg(rest);
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(FILE_BUF) };
    match fs::read_file(&nm[..nl], cwd(), buf) {
        Ok(size) => crate::user::run_elf(&buf[..size]),
        Err(e) => print_err(e),
    }
}

/// `apps`: gömülü tüm C uygulamalarını diske (RFS köküne) yazar ve listeler.
/// Sonrasında `run web`, `run calc`, `run paint` ile diskten çalıştırılabilir.
fn install_c_apps() {
    vga::set_color(Color::LightCyan, Color::Black);
    println!("C uygulamaları diske kuruluyor (RFS):");
    vga::set_color(Color::LightGray, Color::Black);
    let mut installed = 0;
    for (name, elf) in crate::user::c_apps() {
        if elf.len() < 52 {
            println!("  {name:8} - atlandı (gömülü değil; clang ile derleyin)");
            continue;
        }
        match fs::write_file(name.as_bytes(), fs::ROOT, elf) {
            Ok(()) => {
                println!("  {name:8} - kuruldu ({} bayt)", elf.len());
                installed += 1;
            }
            Err(e) => println!("  {name:8} - HATA: {}", e.message()),
        }
    }
    if installed > 0 {
        vga::set_color(Color::LightGreen, Color::Black);
        println!("Tamam. Çalıştırmak için: run web | run calc | run boya | run c4 <dosya.c>");
        println!("İpucu: masaüstünde [+ Kod] ile C yaz, kaydet, sonra: run c4 kod.c");
        vga::set_color(Color::LightGray, Color::Black);
    }
}

/// `kur [ad]`: gömülü örnek ELF'i diske yazar (varsayılan ad: `prog`).
fn elf_install(args: &[char]) {
    let (name, _) = split_arg(args);
    let mut nm = [0u8; 28];
    let nl = if name.is_empty() {
        let d = b"prog";
        nm[..d.len()].copy_from_slice(d);
        d.len()
    } else {
        match name_to_bytes(name, &mut nm) {
            Some(l) => l,
            None => {
                println!("ad hatalı");
                return;
            }
        }
    };
    let elf = crate::user::embedded_elf();
    match fs::write_file(&nm[..nl], cwd(), elf) {
        Ok(()) => {
            vga::set_color(Color::LightGreen, Color::Black);
            if let Ok(s) = core::str::from_utf8(&nm[..nl]) {
                println!(
                    "Gömülü ELF diske yazıldı: '{s}' ({} bayt). 'run {s}' ile çalıştırın.",
                    elf.len()
                );
            }
            vga::set_color(Color::LightGray, Color::Black);
        }
        Err(e) => print_err(e),
    }
}

fn int_test() {
    vga::set_color(Color::LightCyan, Color::Black);
    println!("Kesme (IDT) sınaması:");
    vga::set_color(Color::LightGray, Color::Black);
    println!("  int3 (breakpoint) tetikleniyor...");
    unsafe { core::arch::asm!("int3") };
    vga::set_color(Color::LightGreen, Color::Black);
    println!("  -> istisna yakalandı ve sorunsuz dönüldü (IDT çalışıyor).");
    vga::set_color(Color::DarkGray, Color::Black);
    println!("  Not: gerçek bir hata (örn. geçersiz erişim) olursa çekirdek");
    println!("  panik ekranı gösterip durur (üçlü hata/reset yerine).");
    vga::set_color(Color::LightGray, Color::Black);
}

fn task_test() {
    vga::set_color(Color::LightCyan, Color::Black);
    println!("Çok görevlilik (işbirlikçi) sınaması:");
    vga::set_color(Color::LightGray, Color::Black);
    println!("  3 görev oluşturulup round-robin çalıştırılıyor.");
    println!("  Her görev kendi yığınında çalışır; yield ile sıra değişir.");
    println!("  ----------------------------------------");
    crate::task::run_demo(3);
    println!("  ----------------------------------------");
    vga::set_color(Color::LightGreen, Color::Black);
    println!("  -> tüm görevler bitti; kabuğa dönüldü (bağlam değiştirme çalışıyor).");
    vga::set_color(Color::LightGray, Color::Black);
}

fn user_preempt_test() {
    crate::user::run_preempt_demo();
}

fn sleep_test() {
    vga::set_color(Color::LightCyan, Color::Black);
    println!("Görev uyku (sleep) sınaması:");
    vga::set_color(Color::LightGray, Color::Black);
    println!("  3 görev farklı süreler uyur; timer ile uyanırlar.");
    println!("  ----------------------------------------");
    crate::task::run_sleep_demo();
    println!("  ----------------------------------------");
    vga::set_color(Color::LightGreen, Color::Black);
    println!("  -> sleep/yield ile görevler sırayla uyuyup uyandi.");
    vga::set_color(Color::LightGray, Color::Black);
}

fn preempt_test() {
    vga::set_color(Color::LightCyan, Color::Black);
    println!("Önleyici (preemptive) çok görevlilik sınaması:");
    vga::set_color(Color::LightGray, Color::Black);
    println!("  3 hesap-yoğun görev oluşturuluyor; HİÇBİRİ yield çağırmıyor.");
    println!("  Zamanlayıcı kesmesi (IRQ0) onları ~1.5 sn boyunca zorla böler.");
    println!("  Süre dolunca her görevin yaptığı yineleme sayısı yazılır:");
    println!("  ----------------------------------------");
    crate::task::run_preempt(3, 150);
    println!("  ----------------------------------------");
    vga::set_color(Color::LightGreen, Color::Black);
    println!("  -> üçü de CPU aldı; timer onları kendiliğinden böldü (önleyici zamanlama).");
    vga::set_color(Color::LightGray, Color::Black);
}

fn uptime_info() {
    let ticks = crate::pic::ticks();
    let ms = crate::pic::uptime_ms();
    let secs = ms / 1000;
    let mins = secs / 60;

    vga::set_color(Color::LightCyan, Color::Black);
    println!("Çalışma süresi (donanım zamanlayıcısı):");
    vga::set_color(Color::LightGray, Color::Black);
    println!("  PIT frekansı : {} Hz (IRQ0)", crate::pic::HZ);
    println!("  Tik (tick)   : {ticks}");
    println!("  Süre         : {} dk {} sn", mins, secs % 60);
    vga::set_color(Color::DarkGray, Color::Black);
    println!("  Not: bu sayaç gerçek donanım kesmesiyle (PIT) artar; önleyici");
    println!("  (preemptive) çok görevliliğin temelidir.");
    vga::set_color(Color::LightGray, Color::Black);
}

fn mem_info() {
    let total = crate::mem::total_ram();
    let heap = crate::allocator::heap_size();
    let used = crate::allocator::heap_used();
    let free = heap.saturating_sub(used);
    let frames = crate::mem::frames_free();

    vga::set_color(Color::LightCyan, Color::Black);
    println!("Bellek ve sayfalama:");
    vga::set_color(Color::LightGray, Color::Black);
    println!("  Sayfalama   : AÇIK (birebir/identity, 4 MiB sayfa)");
    println!("  Toplam RAM  : ~{} MiB", total / (1024 * 1024));
    println!("  Yığın (heap): {} MiB", heap / (1024 * 1024));
    println!("    kullanılan: {} KiB", used / 1024);
    println!("    boş       : {} MiB", free / (1024 * 1024));
    println!(
        "  Çerçeve havuzu (4 KiB): {} boş çerçeve (~{} MiB)",
        frames,
        frames * 4096 / (1024 * 1024)
    );
}

fn dev_info() {
    vga::set_color(Color::LightCyan, Color::Black);
    println!("Depolama donanımı:");
    vga::set_color(Color::LightGray, Color::Black);

    // ATA / IDE
    if crate::ata::present() {
        println!("  ATA/IDE   : VAR (kalıcı kullanılabilir)");
    } else {
        println!("  ATA/IDE   : yok");
    }

    // NVMe — gerekiyorsa şimdi algıla.
    if !crate::nvme::detected() {
        crate::nvme::init();
    }
    if crate::nvme::detected() {
        let lba = crate::nvme::lba_bytes();
        let secs = crate::nvme::sectors();
        let mib = (secs as u64 * 512 / (1024 * 1024)) as u32;
        println!("  NVMe      : VAR");
        println!("    blok boyutu : {lba} bayt");
        println!("    kapasite    : {secs} sektör (~{mib} MiB)");
        if lba == 512 {
            vga::set_color(Color::LightGreen, Color::Black);
            println!("    -> 512B blok: KALICI kullanılabilir.");
            println!("       (boş diskse: 'nvmeformat EVET')");
            vga::set_color(Color::LightGray, Color::Black);
        } else {
            vga::set_color(Color::Yellow, Color::Black);
            println!("    -> {lba}B blok: sürücümüz şimdilik yalnızca 512B destekliyor.");
            vga::set_color(Color::LightGray, Color::Black);
        }
    } else {
        println!("  NVMe      : bulunamadı");
        println!("    (SATA/AHCI veya eMMC olabilir; bunlar için sürücümüz yok)");
    }

    // Geçerli arka uç.
    let b = match fs::backend() {
        x if x == fs::BACKEND_ATA => "ATA/IDE (kalıcı)",
        x if x == fs::BACKEND_NVME => "NVMe (kalıcı)",
        _ => "RAM diski (kalıcı DEĞİL)",
    };
    vga::set_color(Color::LightCyan, Color::Black);
    println!("Şu an kullanılan: {b}");
    vga::set_color(Color::LightGray, Color::Black);
}

/// NVMe diskini biçimlendirip kalıcı arka uç yapar. Yıkıcı olduğu için açık
/// onay ister: `nvme-biçimlendir EVET`.
fn nvme_format(args: &[char]) {
    let (s, e) = trim(args);
    let arg = &args[s..e];
    if !(eq(arg, "EVET") || eq(arg, "evet")) {
        vga::set_color(Color::LightRed, Color::Black);
        println!("DİKKAT: NVMe diskinin ilk {DISK_BLOCKS} bloğunu (2 MiB) biçimlendirir;");
        println!("o bölgedeki TÜM veri silinir! Gerçek bir diskse veri kaybına yol açar.");
        vga::set_color(Color::LightGray, Color::Black);
        println!("Onaylamak için:  nvmeformat EVET");
        return;
    }
    if !crate::nvme::ready() && !crate::nvme::init() {
        vga::set_color(Color::LightRed, Color::Black);
        println!("NVMe denetleyicisi bulunamadı (ya da 512B blok değil).");
        vga::set_color(Color::LightGray, Color::Black);
        return;
    }
    fs::set_backend(fs::BACKEND_NVME);
    let blocks = core::cmp::min(crate::nvme::sectors(), DISK_BLOCKS);
    match fs::format(blocks) {
        Ok(()) => {
            crate::seed_files();
            vga::set_color(Color::LightGreen, Color::Black);
            println!("NVMe biçimlendirildi (RFS, kalıcı). Dosyalar artık NVMe'de tutuluyor.");
            vga::set_color(Color::LightGray, Color::Black);
        }
        Err(err) => {
            fs::set_backend(fs::BACKEND_RAM);
            print_err(err);
        }
    }
}

fn fs_df() {
    match fs::usage() {
        Ok((used, total)) => {
            println!(
                "disk: {used}/{total} blok dolu  (~{} / {} KiB)",
                used * 512 / 1024,
                total * 512 / 1024
            );
        }
        Err(e) => print_err(e),
    }
}

/// İstemi (prompt) renkleriyle yazar.
fn prompt() {
    let dt = crate::rtc::now();
    vga::set_color(Color::DarkGray, Color::Black);
    print!(
        "[{:02}.{:02} {:02}:{:02}] ",
        dt.day, dt.month, dt.hour, dt.min
    );
    vga::set_color(Color::LightGreen, Color::Black);
    print!("{PROMPT}");
    print_cwd_path();
    print!("> ");
    vga::set_color(Color::LightGray, Color::Black);
}

/// Kök '/' ile başlayan geçerli dizin yolunu yazar (örn. "/belgeler/2026").
fn print_cwd_path() {
    let dir = cwd();
    if dir == fs::ROOT {
        print!(":/");
        return;
    }
    // Üst dizinleri toplayıp ters sırada bas. En çok MAX_FILES derinlik.
    let mut chain = [0u8; fs::MAX_FILES];
    let mut n = 0;
    let mut cur = dir;
    while cur != fs::ROOT && n < chain.len() {
        chain[n] = cur;
        n += 1;
        cur = fs::parent_of(cur).unwrap_or(fs::ROOT);
    }
    print!(":");
    for i in (0..n).rev() {
        print!("/");
        if let Ok(Some(info)) = fs::entry_info(chain[i] as usize) {
            print_name(&info.name);
        }
    }
}

/// Grafik masaüstüne geçer ve dönünce terminali temiz bir duruma getirir.
///
/// Henüz grafik mod yoksa (QEMU -kernel / metin modu) Bochs VBE ile çalışma
/// anında grafik moduna geçer ve grafik konsolu (`fbcon`) başlatır. Bundan
/// sonra terminal de grafik konsolda çizilir; böylece kırılgan VGA-metin
/// geri yüklemesine gerek kalmaz. GRUB ile açıldıysa zaten grafik moddadır.
///
/// Dönüş değeri: metin modundan grafiğe YENİ geçildiyse `true` (çağıran tarafın
/// istemi/satırı yeniden basması gerekir). Zaten grafik moddaysak `false`
/// (terminal aynen geri çizilmiştir).
fn enter_gui() -> bool {
    if crate::gfx::is_active() {
        // Zaten grafik mod (GRUB ya da daha önce VBE ile geçilmiş).
        crate::gui::run();
        crate::fbcon::redraw();
        return false;
    }

    // Metin modu: Bochs VBE ile çalışma anında grafik moduna geç.
    if !crate::gfx::enable_bochs(1024, 768, 32) {
        vga::set_color(Color::LightRed, Color::Black);
        println!("grafik moduna geçilemedi (VBE desteklenmiyor).");
        vga::set_color(Color::LightGray, Color::Black);
        return false;
    }
    crate::fbcon::init();
    crate::mouse::init(crate::gfx::width(), crate::gfx::height());
    crate::gui::run();
    crate::fbcon::clear(); // masaüstünü temizle; taze grafik terminal
    true
}

/// 8042 klavye denetleyicisi üzerinden CPU'yu yeniden başlatır.
fn reboot() -> ! {
    println!("yeniden başlatılıyor...");
    unsafe {
        crate::port::outb(0x64, 0xFE);
    }
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}
