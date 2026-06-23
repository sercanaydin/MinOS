# MinOS — Giriş Seviyesi İşletim Sistemi

Rust + Assembly ile sıfırdan yazılmış, **Multiboot** uyumlu, 32-bit x86 için
küçük bir işletim sistemi çekirdeği. Açıldığında ekrana yazı basar, klavyeden
girdi okur ve basit bir **kabuk (shell)** sunar.

![MinOS ekran görüntüsü](docs/screenshot.png)

## Özellikler

- `no_std` Rust çekirdeği (standart kütüphane yok, işletim sistemi *biziz*)
- Assembly ile yazılmış Multiboot v1 başlığı ve giriş noktası (`src/boot.asm`)
- VGA metin modu sürücüsü: renkler, kaydırma (scroll), donanım imleci
- PS/2 klavye sürücüsü (kesmesiz, sürekli yoklama / *polling*) — **Türkçe-Q düzeni**
- Türkçe harf desteği: `ş ğ ı İ Ş Ğ` glifleri, VGA fontuna çalışma anında üretilip yüklenir
- **Dosya sistemi (RFS):** kendi basit formatımız + **dizin (klasör) desteği**;
  ATA/IDE, NVMe ve RAM diski arka uçları (ATA/NVMe kalıcı)
- **Grafik masaüstü (GUI):** framebuffer çizimi, fare imleci, dosya ve **klasör
  ikonları** — dosyaya tıklayınca içeriği pencerede açılır, klasöre tıklayınca
  içine girilir
- **PS/2 fare sürücüsü** ve klavye/fareyi tek noktadan yöneten birleşik giriş katmanı
- Gömülü 8x8 bitmap font ile grafik modda yazı çizimi (Türkçe harfler dahil)
- `F1` tuşuyla **terminal ↔ masaüstü** arasında geçiş
- Etkileşimli kabuk ve komutlar: `help`, `echo`, `clear`, `about`, `colors`, `turkce`, `gui`, `web`, dosya komutları, `reboot`
- Ağ ve internet: Intel e1000 sürücüsü + smoltcp TCP/IP yığını (DHCP, DNS, TCP, HTTP). `web <adres>` ile metin tabanlı sayfa çekme
- Hiyerarşik dizinler: `mkdir`, `cd`, `pwd` ile alt klasörler (üst-işaretçi modeli; eski biçimle uyumlu)
- Hata ayıklama için COM1 seri port çıktısı

## Gereksinimler

macOS (Homebrew) için:

```bash
brew install rustup nasm qemu
rustup toolchain install nightly
rustup component add rust-src llvm-tools --toolchain nightly
```

Homebrew'in `rustup`'ı "keg-only" olduğundan PATH'in başına eklemeniz gerekir:

```bash
echo 'export PATH="/opt/homebrew/opt/rustup/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

> Projedeki `rust-toolchain.toml`, derlemede otomatik olarak nightly + `rust-src`
> kullanılmasını sağlar.

## Derleme ve Çalıştırma

**Tek komut yeter:**

```bash
make run          # TEK ortam: UEFI + grafik + NVMe diski. Her şey burada.
```

`make run` açıldığında her şeyi terminal komutlarıyla yaparsınız:

| Komut | Ne yapar |
|-------|----------|
| `help` | tüm komutları listeler |
| `gui` veya **F1** | grafik masaüstüne geçer |
| `saat` | tarih ve saati gösterir (gün adıyla) |
| `depo` | depolama donanımını gösterir (NVMe/ATA, blok boyutu) |
| `ls`, `cat`, `yaz`, `rm` | dosya işlemleri |
| `mkdir`, `cd`, `pwd` | dizin işlemleri (alt klasör desteği) |
| `web <adres>` | internetten bir sayfa çeker (metin tarayıcı) |
| `nvmeformat EVET` | NVMe'yi kalıcı yapar (boş diskte güvenli) |

> **Dosyalar:** Varsayılan olarak dosyalar RAM'de tutulur (örnek dosyalar hazır
> gelir, ama yeniden başlatınca silinir). Kalıcı istiyorsanız bir kez
> `nvmeformat EVET` deyin; sonraki açılışlarda NVMe'den otomatik olarak kalıcı
> bağlanır.

Diğer (ileri/opsiyonel) hedefler:

```bash
make build        # sadece derler
make run-text     # hızlı metin modu (QEMU -kernel; F1 ile grafiğe geçilir)
make run-full     # tam ekran
make run-serial   # COM1 çıktısını terminale yansıtır
make iso-uefi     # gerçek donanım için USB'ye yazılabilir hibrit ISO
make clean        # derleme çıktılarını temizler
```

> **Pencere küçük mü?** `-display cocoa,zoom-to-fit=on` kullanılır; pencereyi
> köşesinden büyütünce içerik ölçeklenir, ya da `make run-full` ile tam ekran açın.

`make` kullanmadan doğrudan da çalıştırabilirsiniz:

```bash
cargo build
qemu-system-i386 -kernel target/i686-os/debug/rustos
```

QEMU penceresi açıldığında `help` yazıp Enter'a basın.

> **Not:** Ham `cargo` komutlarının çalışması için rustup'ın PATH'inizde olması
> ve nightly'yi seçmesi gerekir (yukarıdaki kurulum adımları bunu sağlar).
> En garantili yol `make` kullanmaktır; çünkü `Makefile`, rustup'ın nightly
> cargo'sunu tam yoluyla ve açıkça çağırır.

> QEMU penceresinden çıkmak için: menüden **Stop**, ya da `Ctrl` tuşuyla
> pencereyi kapatın. (Fareyi yakalarsa `Ctrl+Alt+G` ile bırakır.)

### Sanal makineye / gerçek donanıma kurmak (önyüklenebilir ISO)

`make run`, QEMU'ya özel `-kernel` kısayolunu kullanır; bu yüzden tek başına
VirtualBox/VMware gibi bir sanal makinede **çalışmaz.** Bağımsız çalışması için
GRUB içeren önyüklenebilir bir ISO üretmek gerekir. Bunun için gereken araçlar:

```bash
brew install i686-elf-grub xorriso mtools
```

Ardından:

```bash
make iso          # build/rustos.iso üretir (her yerde açılır)
make run-iso      # ISO'yu GRUB ile (—kernel OLMADAN) QEMU'da dener
```

Üretilen `build/rustos.iso` hibrit bir görüntüdür ve şuralarda açılır:

- **QEMU:** `qemu-system-i386 -cdrom build/rustos.iso`
- **VirtualBox / VMware:** Yeni bir VM oluşturup (tip: Other/Unknown, 32-bit)
  bu ISO'yu CD/DVD olarak takın ve başlatın.
- **Gerçek donanım / USB:** `dd if=build/rustos.iso of=/dev/diskX` ile bir USB'ye
  yazıp o makineyi USB'den açabilirsiniz. (Dosya sistemini kullanmak için
  bilgisayarda bir IDE/SATA disk gerekir.)

> Not: GRUB ISO'su yalnızca **Legacy BIOS / CSM** ile açılır. Modern UEFI-only
> makineler için aşağıdaki Limine hibrit ISO'yu kullanın.

### Modern donanım: UEFI + BIOS hibrit ISO (Limine)

Modern laptoplar genelde **UEFI-only**'dir; yukarıdaki GRUB ISO'su orada açılmaz.
Hem UEFI hem BIOS ile açılan bir hibrit ISO üretmek için **Limine** kullanıyoruz:

```bash
brew install limine xorriso mtools

make iso-uefi     # build/rustos-uefi.iso  (UEFI + BIOS, hibrit)
make run-uefi     # gerçek UEFI firmware (OVMF) ile QEMU'da dener
make run-bios-limine  # aynı ISO'yu Legacy BIOS ile QEMU'da dener
make run-nvme     # UEFI (OVMF) + emüle NVMe SSD ile dener (kalıcı NVMe testi)
```

`build/rustos-uefi.iso` şuralarda açılır:

- **Gerçek UEFI laptop / PC:** ISO'yu USB'ye yazın ve makineyi USB'den açın:
  ```bash
  diskutil list                      # USB'nin /dev/diskN adını bulun (DİKKAT!)
  diskutil unmountDisk /dev/diskN
  sudo dd if=build/rustos-uefi.iso of=/dev/rdiskN bs=4m
  ```
  Firmware'de **Secure Boot'u kapatın** (imzasız bootloader) ve gerekiyorsa
  USB'yi önyükleme sırasına alın. Açılışta grafik masaüstüne düşer.
- **VirtualBox / VMware:** ISO'yu CD/DVD olarak takın. (VirtualBox'ta
  Ayarlar → System → "Enable EFI" ile UEFI modunu da deneyebilirsiniz.)

### Kalıcı depolama: ATA/IDE, NVMe ve RAM diski

Sistem açılışta sırayla şu blok aygıtlarını dener:

1. **ATA/IDE** (QEMU `if=ide`, VirtualBox IDE) — varsa kalıcı.
2. **NVMe** (PCIe SSD; modern UEFI laptopların diski) — yalnızca üzerinde
   **zaten bizim dosya sistemimiz (RFS)** varsa otomatik kullanılır.
3. **RAM diski** — hiçbiri yoksa son çare. Masaüstü ve tüm dosya komutları
   çalışır, ama veriler **yeniden başlatınca silinir**.

> ⚠️ **GÜVENLİK — gerçek laptopunda dikkat:** NVMe diskin senin asıl işletim
> sistemini (Windows/macOS/Linux) barındırır. Bu yüzden sistem boş/yabancı bir
> NVMe diskini **asla otomatik biçimlendirmez** — biçimlendirmek o bölgedeki
> **tüm veriyi siler**. Kalıcı kullanmak istersen, bunu **bilerek** açık komutla
> yaparsın:
>
> ```
> nvmeformat EVET      # NVMe'nin ilk 2 MiB'ını RFS olarak biçimlendirir (YIKICI!)
> ```
>
> Bu yüzden günlük laptopunda kalıcı depolamayı **yalnızca boş/ayrılmış bir
> NVMe diskinde** kullan; ana SSD'ne dokunma. QEMU `make run-nvme` ve boş NVMe
> diskler için ise kalıcı depolama tam çalışır (test edilmiştir).

NVMe sürücüsü 32-bit çekirdek için yazıldığından, UEFI firmware'inin 64-bit MMIO
BAR'ı 4 GB üstüne koyması durumunda BAR'ı 4 GB altında bilinen bir adrese
(`0xFE000000`) yeniden programlar; aksi halde 32-bit çekirdek erişemezdi.

## Grafik Masaüstü (GUI)

Dosya sistemini bir masaüstünde gösteren basit bir grafik arayüz içerir.
Bir **framebuffer** (örn. 1024×768, 32-bit renk) üzerinde her şeyi (yazı, ikon,
fare imleci, pencere) piksel piksel kendimiz çizeriz.

![Masaüstü](docs/gui_desktop.png)

- **Açmak için:** terminalde **`F1`**'e basın ya da `gui` yazın.
- **Dosya açmak:** bir dosya ikonuna **tıklayın**; içerik bir pencerede açılır.
  Pencereyi kırmızı **[X]** ile kapatın.
- **Dizinler:** klasörler sarı ikonla gösterilir. Bir klasöre tıklayınca içine
  girersiniz; sol üstteki gri **`..`** ikonuyla üst dizine çıkarsınız. Alt
  çubuk bulunulan dizin yolunu (`Konum: /belgeler` gibi) gösterir.
- **Geri dönmek:** `F1` veya `ESC` ile terminale dönersiniz.

![Dosya penceresi](docs/gui_window.png)

> **İki framebuffer yolu vardır:**
> - **Önyükleyici ile (ISO / gerçek donanım — GRUB veya Limine, `make run`):**
>   önyükleyici framebuffer'ı kurar; taşınabilir yoldur.
> - **Metin modunda (`make run-text`, QEMU `-kernel`):** `F1`'e basınca çekirdek,
>   standart VGA aygıtının **Bochs‑VBE** yazmaçlarını programlayıp framebuffer'ı
>   **çalışma anında** kendisi açar (LFB adresini PCI'dan bulur). Geri dönüşte
>   terminal yine framebuffer üzerinde (grafik konsol) gösterilir; böylece
>   kırılgan VGA‑metin geri yüklemesine gerek kalmaz. QEMU ve VirtualBox'ın
>   Bochs‑VBE adaptöründe çalışır.
>
> Masaüstü, ilk biçimlendirmede oluşturulan örnek dosyalarla dolu gelir
> (`make clean-disk` sonrası yeniden üretilir).

## Proje Yapısı

| Dosya | Görevi |
|-------|--------|
| `src/boot.asm` | Multiboot başlığı, yığın ve `_start` giriş noktası |
| `src/main.rs` | `kernel_main` ve panik işleyici |
| `src/vga.rs` | VGA metin modu sürücüsü + `print!` yönlendirme |
| `src/gfx.rs` | Framebuffer (grafik) sürücüsü: piksel/dikdörtgen/çizgi |
| `src/fbcon.rs` | Grafik modda metin konsolu (8x8 fontla çizer) |
| `src/font_data.rs` | Gömülü 8x8 bitmap font + Türkçe glif üretimi |
| `src/gui.rs` | Masaüstü, ikonlar, fare imleci ve dosya penceresi |
| `src/mouse.rs` | PS/2 fare sürücüsü (yoklama) |
| `src/input.rs` | Birleşik giriş: 8042 baytlarını klavye/fareye yönlendirir |
| `src/keyboard.rs` | PS/2 klavye (Türkçe-Q) + scancode → karakter |
| `src/font.rs` | VGA metin fontuna Türkçe harfleri üretip yükler |
| `src/ata.rs` | ATA/IDE disk sürücüsü (PIO, sektör oku/yaz) |
| `src/nvme.rs` | NVMe (PCIe SSD) sürücüsü: admin/IO kuyrukları, sektör oku/yaz |
| `src/ramdisk.rs` | RAM tabanlı blok aygıtı (disk yoksa yedek; kalıcı değil) |
| `src/fs.rs` | RFS dosya sistemi (superblock, bitmap, dizin; ATA/NVMe/RAM arka ucu) |
| `src/shell.rs` | Komut satırı kabuğu |
| `src/serial.rs` | COM1 seri port (hata ayıklama) |
| `src/rtc.rs` | CMOS gerçek zaman saati (tarih/saat) okuyucu |
| `src/allocator.rs` | Yığın (heap) kurulumu — `alloc` için global ayırıcı |
| `src/time.rs` | Monotonik ms saati (TSC, PIT ile kalibre; ağ zaman aşımları) |
| `src/e1000.rs` | Intel e1000 Ethernet sürücüsü (RX/TX DMA halkaları, yoklama) |
| `src/net.rs` | smoltcp TCP/IP yığını: DHCP + DNS + TCP + HTTP GET |
| `src/port.rs` | `in`/`out` port G/Ç komutları (8/16/32-bit) |
| `src/pci.rs` | Küçük PCI tarayıcı: framebuffer, NVMe ve e1000'i bulur |
| `i686-os.json` | Özel bare-metal derleme hedefi |
| `linker.ld` | Bellek yerleşimi (1 MiB'den başlar) |
| `build.rs` | `boot.asm`'i NASM ile derler, linker betiğini bağlar |
| `grub.cfg` | GRUB menü girdisi (ISO için) |

## Açılış Akışı

```
GRUB / QEMU  ──►  boot.asm (_start)  ──►  kernel_main()  ──►  shell::run()
   (Multiboot)     yığını kurar          ekranı hazırlar     komutları işler
                   kernel_main'i çağırır
```

## Nasıl Çalışıyor? (kısa notlar)

- **Multiboot:** `boot.asm` içindeki sihirli sayı (`0x1BADB002`) sayesinde
  önyükleyici çekirdeği tanır ve 32-bit korumalı kipte, sayfalama kapalı olarak
  bize devreder.
- **VGA:** `0xB8000` adresindeki metin tamponuna her karakter 2 bayt (karakter +
  renk) olarak yazılır.
- **Klavye:** Kesme altyapısı kurmadan, `0x64` durum portu yoklanır ve `0x60`
  veri portundan tarama kodları okunur.
- **Framebuffer:** GRUB grafik modu verdiğinde, Multiboot bilgi yapısından
  tampon adresi/çözünürlük okunur ve piksellere doğrudan yazılır. Yazılar gömülü
  8x8 fontla, masaüstü ise basit dikdörtgenlerle çizilir.
- **Birleşik giriş:** Klavye ve fare aynı 8042 tamponunu paylaştığı için baytlar
  tek noktadan (`input::poll`) okunur ve "aux" bitine göre ayrıştırılır.
- **Yığın (heap):** Çekirdek statik bir bellek bölgesini global ayırıcıya verir
  (`src/allocator.rs`), böylece `alloc` etkindir ve ağ yığını gibi yerlerde
  `String`/`Vec` kullanılabilir. Çoğu çekirdek yolu yine de sabit tamponlarla
  çalışır.

## Dosya Sistemi (RFS)

Diske yazan basit bir dosya sistemi içerir. Hangi blok aygıtının kullanıldığına
göre kalıcılık değişir (yukarıdaki "Kalıcı depolama" bölümüne bakın): ATA/IDE
ya da biçimlenmiş NVMe **kalıcıdır** (`make run-text`'teki `disk.img` gibi),
RAM diski ise **kalıcı değildir** (`make run`'ın varsayılanı).

![Dosya sistemi](docs/filesystem.png)

İlk açılışta disk boşsa otomatik biçimlendirilir. Komutlar:

| Komut | Açıklama |
|-------|----------|
| `ls` | Dosyaları ve boyutlarını listeler |
| `cat <ad>` | Dosya içeriğini gösterir |
| `yaz <ad> <metin>` | Dosyaya yazar (üzerine; `write` de olur) |
| `ekle <ad> <metin>` | Dosyanın sonuna ekler (`append`) |
| `rm <ad>` | Dosyayı veya boş dizini siler |
| `mkdir <ad>` | Yeni dizin (klasör) oluşturur |
| `cd <ad>` / `cd ..` / `cd /` | Dizine girer / üste çıkar / köke döner |
| `pwd` | Bulunulan dizin yolunu gösterir |
| `df` | Disk kullanımını gösterir |
| `format` | Diski sıfırlar (TÜM veri silinir) |

Örnek:

```
os:/> mkdir belgeler
dizin oluşturuldu
os:/> cd belgeler
os:/belgeler> yaz notlar merhaba dünya
tamam (14 bayt)
os:/belgeler> ls
notlar  (14 bayt)
os:/belgeler> cat notlar
merhaba dünya
os:/belgeler> cd ..
os:/>
```

### Disk düzeni

| Blok | İçerik |
|------|--------|
| 0 | Superblock (sihirli sayı, sürüm, toplam blok) |
| 1 | Blok ayırma haritası (bitmap), 1 bit = 1 blok |
| 2–9 | Girdi tablosu: 8 blok × 4 girdi = 32 girdi (dosya + dizin) |
| 10+ | Veri blokları |

Her dosya en çok 22 doğrudan bloğa (≈ 11 KiB) sahip olabilir. **Dizinler
desteklenir:** her girdi `kind` (dosya/dizin) ve `parent` (üst dizin) bilgisini
tutar — "üst-işaretçi" modeli. Bu alanlar girdideki kullanılmayan dolgu
baytlarına yerleştirildiğinden eski biçimlenmiş diskler de uyumludur (eski
dosyalar otomatik kök dizinde görünür). Disk imajı varsayılan 2 MiB'tır
(`make disk`).

> Kalıcı verileri sıfırlamak için: `make clean-disk` (disk imajını siler).

## Sık Karşılaşılan Sorun

```
error: `.json` target specs require -Zjson-target-spec to be added to the cargo invocation
```

Bu hata, derlemenin **nightly yerine Homebrew'in stable cargo'su** ile yapıldığı
anlamına gelir (özel `.json` hedefi nightly gerektirir). Çözüm: `make` kullanın
(önerilir) veya rustup'ı PATH'inizin başına ekleyip nightly'yi kullanın:

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo +nightly build
```

## Sonraki Adımlar (öğrenmeye devam)

- Kesme tablosu (IDT) + PIC kurup klavyeyi *interrupt* tabanlı okumak
- Zamanlayıcı (PIT) ile kesme alıp saat/uptime göstermek
- Sayfalama (paging) eklemek
- Çok görevlilik (çok basit bir zamanlayıcı)
- TLS desteği ekleyip HTTPS sayfaları açmak
- GUI'de dosya oluşturma/silme ve çok parçalı yol (`cd a/b/c`)

> Tamamlananlar: yığın (heap) ayırıcı, dizin (klasör) desteği — `mkdir`/`cd`/`pwd`
> ve GUI'de klasör gezintisi, ağ/internet (e1000 + smoltcp ile `web`).

## Lisans

[MIT](LICENSE) — özgürce kullanabilir, değiştirebilir ve dağıtabilirsiniz.
