//! MinOS — giriş seviyesi, Rust + Assembly ile yazılmış bir çekirdek.
//!
//! Açılış akışı:
//!   GRUB/QEMU  ->  boot.asm (_start)  ->  kernel_main (bu dosya)  ->  shell
#![no_std]
#![no_main]

extern crate alloc;

mod allocator;
mod ata;
mod e1000;
mod fbcon;
mod net;
mod font;
mod font_data;
mod fs;
mod gfx;
mod gui;
mod input;
mod keyboard;
mod mouse;
mod nvme;
mod pci;
mod port;
mod ramdisk;
mod rtc;
mod serial;
mod shell;
mod time;
mod vga;

use core::panic::PanicInfo;
use vga::Color;

/// Çekirdeğin Rust tarafındaki giriş noktası. boot.asm tarafından çağrılır.
///
/// `magic` Multiboot sihirli sayısı (0x2BADB002 olmalı), `_mbi` ise GRUB'ın
/// bıraktığı bilgi yapısının adresidir.
#[no_mangle]
pub extern "C" fn kernel_main(magic: u32, mbi: u32) -> ! {
    serial::init();
    serial::write_str("[boot] kernel_main calisti\n");

    // Yığını (heap) en başta kur; sonraki her şey alloc kullanabilir.
    allocator::init();
    // Monotonik saat (TSC) kalibrasyonu — ağ zaman aşımları için.
    time::init();

    // GRUB grafik modu verdiyse framebuffer konsolunu, vermediyse VGA metin
    // modunu kullanırız. (QEMU'nun -kernel modu framebuffer kurmaz; ISO/GRUB
    // ile veya gerçek donanımda grafik mod devreye girer.)
    if gfx::init(mbi) {
        serial::write_str("[gfx] framebuffer aktif; grafik konsol\n");
        fbcon::init();
        mouse::init(gfx::width(), gfx::height());
    } else {
        serial::write_str("[gfx] framebuffer YOK; VGA metin modu\n");
        font::install_turkish();
        vga::clear();
    }

    // Karşılama başlığı.
    vga::set_color(Color::Yellow, Color::Blue);
    println!(" MinOS v0.1 - giriş seviyesi işletim sistemi          ");
    vga::set_color(Color::LightGray, Color::Black);
    println!();

    if magic == 0x2BADB002 {
        println!("[ok] Multiboot ile açıldı (magic doğrulandı).");
    } else {
        vga::set_color(Color::LightRed, Color::Black);
        println!("[uyarı] Multiboot magic beklenenden farklı: {magic:#x}");
        vga::set_color(Color::LightGray, Color::Black);
    }

    mount_disk();

    // Ağ kartı (e1000) varsa başlat.
    if e1000::init() {
        println!("[ok] e1000 ağ kartı bulundu. ('web <adres>' ile internete bağlanın)");
    } else {
        serial::write_str("[e1000] kart bulunamadi\n");
    }

    println!("Komutları görmek için 'help' yazın.");
    vga::set_color(Color::LightCyan, Color::Black);
    println!(">> Grafik masaüstü için F1'e basın ya da 'gui' yazın.");
    vga::set_color(Color::LightGray, Color::Black);
    println!();

    serial::write_str("[boot] shell baslatiliyor\n");
    shell::run();
}

/// Dosya sistemini hazırlar. Önce gerçek (kalıcı) bir ATA diski dener; yoksa
/// ya da başarısız olursa RAM tabanlı bir diske düşer (kalıcı değil ama her
/// donanımda — UEFI/NVMe makineler dahil — masaüstü ve komutlar çalışır).
fn mount_disk() {
    // 1) ATA/IDE diski (QEMU disk.img, VirtualBox IDE).
    if ata::present() {
        fs::set_backend(fs::BACKEND_ATA);
        match fs::mount() {
            Ok(()) => {
                println!("[ok] ATA diski bağlandı (RFS, kalıcı).");
                return;
            }
            Err(fs::FsError::NotFormatted) => {
                println!("[bilgi] boş ATA diski; biçimlendiriliyor...");
                if fs::format(shell::DISK_BLOCKS).is_ok() {
                    println!("[ok] disk biçimlendirildi (RFS, kalıcı).");
                    seed_files();
                    return;
                }
            }
            Err(_) => {}
        }
    }

    // 2) NVMe (PCIe SSD). GÜVENLİK: yalnızca üzerinde ZATEN bizim RFS'imiz
    // varsa otomatik kullanılır. Aksi halde (gerçek bir disk olabilir!) ASLA
    // otomatik biçimlendirilmez; kullanıcı bilerek 'nvme-biçimlendir' demeli.
    if nvme::init() {
        fs::set_backend(fs::BACKEND_NVME);
        if fs::mount().is_ok() {
            println!("[ok] NVMe diski bağlandı (RFS, kalıcı).");
            return;
        }
        vga::set_color(Color::Yellow, Color::Black);
        println!("[bilgi] NVMe bulundu ama üzerinde RFS yok; güvenlik için dokunulmuyor.");
        println!("        Kalıcı kullanmak için 'nvmeformat EVET' (TÜM NVMe verisini siler!).");
        vga::set_color(Color::LightGray, Color::Black);
    }

    // 3) Hiçbiri yoksa: RAM diski (kalıcı değil ama her donanımda çalışır).
    fs::set_backend(fs::BACKEND_RAM);
    vga::set_color(Color::Yellow, Color::Black);
    println!("[bilgi] kalıcı disk yok; RAM diski kullanılıyor (yeniden başlatınca silinir).");
    vga::set_color(Color::LightGray, Color::Black);
    match fs::format(ramdisk::SECTORS) {
        Ok(()) => seed_files(),
        Err(e) => {
            vga::set_color(Color::LightRed, Color::Black);
            println!("[hata] RAM diski biçimlendirilemedi: {}", e.message());
            vga::set_color(Color::LightGray, Color::Black);
        }
    }
}

/// İlk biçimlendirmeden sonra birkaç örnek dosya oluşturur. Böylece
/// masaüstü boş açılmaz ve dosya sistemi hemen denenebilir.
pub fn seed_files() {
    let samples: [(&str, &str); 2] = [
        (
            "oku-beni.txt",
            "MinOS'a hos geldin!\n\nBu dosya sistemi kalicidir; yeniden baslatinca korunur.\nMasaustunde bir ikona tiklayarak icerigini gorebilirsin.\n",
        ),
        (
            "turkce.txt",
            "Pijamali hasta yagiz sofore cabucak guvendi.\ncgiosu CGIOSU test satiri.\n",
        ),
    ];
    // Örnek bir dizin de oluştur (dizin sistemi gösterimi için).
    let _ = fs::mkdir(b"belgeler", fs::ROOT);
    for (name, body) in samples {
        let _ = fs::write_file(name.as_bytes(), fs::ROOT, body.as_bytes());
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial::write_str("[panik] cekirdek paniye girdi\n");
    vga::set_color(Color::White, Color::Red);
    println!();
    if let Some(loc) = info.location() {
        println!("CEKIRDEK PANIGI ({}:{})", loc.file(), loc.line());
    } else {
        println!("CEKIRDEK PANIGI");
    }
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}
