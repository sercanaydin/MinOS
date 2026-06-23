# RustOS — derleme ve çalıştırma yardımcıları.
#
# rustup'ı (ve dolayısıyla nasm/qemu'nun bulunduğu Homebrew dizinini) çocuk
# süreçlerin ortamına ekliyoruz.
export PATH := /opt/homebrew/opt/rustup/bin:$(PATH)

# Önemli: Homebrew rustup keg-only olduğundan ve varsayılan toolchain "stable"
# (Homebrew rust) olduğundan, cargo'yu rustup proxy'sinin TAM YOLU üzerinden ve
# açıkça `+nightly` ile çağırıyoruz. (Mutlak yol, GNU make'in meta-karaktersiz
# satırları kabuk yerine doğrudan çalıştırıp yanlış cargo'yu seçmesini önler.)
RUSTUP_BIN := /opt/homebrew/opt/rustup/bin
CARGO      := $(RUSTUP_BIN)/cargo +nightly
KERNEL     := target/i686-os/debug/rustos
QEMU       := qemu-system-i386

# Kalıcı dosya sistemi için disk imajı (2 MiB = 4096 sektör).
DISK      := disk.img
DISK_SIZE := 2M
DISK_OPTS := -drive file=$(DISK),format=raw,if=ide

# Emüle NVMe SSD imajı (4 MiB).
NVME_IMG  := build/nvme.img

.PHONY: all build run run-text run-full run-serial disk clean clean-disk iso run-iso gui iso-uefi run-uefi run-bios-limine run-nvme

all: build

## build: çekirdeği derler
build:
	$(CARGO) build

# Pencere içeriğini, pencereyi büyüttükçe ölçekleyerek doldurur (cocoa/macOS).
DISPLAY_OPTS := -display cocoa,zoom-to-fit=on
# Grafik (framebuffer) modu için standart VGA aygıtı (VBE doğrusal tampon).
VGA_OPTS := -vga std
# Ağ kartı: Intel e1000 + QEMU kullanıcı modu ağı (NAT, DHCP, DNS). 'web' komutu
# bununla internete çıkar. (Ana makinenin internet bağlantısını kullanır.)
NET_OPTS := -netdev user,id=net0 -device e1000,netdev=net0

## disk: dosya sistemi için disk imajını (yoksa) oluşturur
disk: $(DISK)
$(DISK):
	qemu-img create -f raw $(DISK) $(DISK_SIZE)

## run: TEK ve eksiksiz ortam. UEFI + grafik + NVMe diski.
##      Açılınca her şey terminal komutlarıyla yapılır:
##        help         - tüm komutlar
##        gui / F1     - grafik masaüstü
##        depo         - depolama donanımı (NVMe/ATA)
##        nvmeformat   - NVMe'yi kalıcı yap (boş diskte güvenli)
##      Varsayılan: dosyalar RAM'de (örnek dosyalar hazır gelir). Kalıcı istersen
##      'nvmeformat EVET' ile NVMe'ye geç; sonraki açılışlarda otomatik kalıcı.
run: iso-uefi
	@mkdir -p build
	@test -f $(NVME_IMG) || qemu-img create -f raw $(NVME_IMG) 4M
	@test -f build/ovmf-vars.fd || cp $(OVMF_VARS_TPL) build/ovmf-vars.fd
	qemu-system-x86_64 -machine q35 -m 256M \
		-drive if=pflash,format=raw,unit=0,readonly=on,file=$(OVMF) \
		-drive if=pflash,format=raw,unit=1,file=build/ovmf-vars.fd \
		-cdrom $(UEFI_ISO) \
		-drive file=$(NVME_IMG),if=none,id=nvm,format=raw \
		-device nvme,serial=rustos01,drive=nvm \
		-rtc base=localtime \
		$(NET_OPTS) $(DISPLAY_OPTS) $(VGA_OPTS)

## gui / run-nvme: 'run' ile aynı tek ortama yönlenir (geriye dönük uyumluluk)
gui: run
run-nvme: run

## run-text: hızlı, basit metin modu (QEMU -kernel; F1 ile grafiğe geçilir, NVMe yok)
run-text: build $(DISK)
	$(QEMU) $(DISPLAY_OPTS) $(VGA_OPTS) $(NET_OPTS) -rtc base=localtime -kernel $(KERNEL) $(DISK_OPTS)

## run-full: tam ekran başlatır (metin modu, -kernel)
run-full: build $(DISK)
	$(QEMU) -display cocoa,zoom-to-fit=on,full-screen=on $(VGA_OPTS) -kernel $(KERNEL) $(DISK_OPTS)

## run-serial: ekrandakileri ayrıca terminale (COM1) yansıtarak başlatır
run-serial: build $(DISK)
	$(QEMU) $(DISPLAY_OPTS) $(VGA_OPTS) $(NET_OPTS) -kernel $(KERNEL) $(DISK_OPTS) -serial stdio

# grub-mkrescue yoksa Homebrew'in çapraz sürümünü (i686-elf-...) kullan.
GRUB_MKRESCUE := $(shell command -v grub-mkrescue 2>/dev/null || command -v i686-elf-grub-mkrescue 2>/dev/null)

## iso: GRUB ile önyüklenebilir bir .iso üretir (her VM'de/gerçek donanımda açılır)
iso: build
	@test -n "$(GRUB_MKRESCUE)" || { echo "HATA: grub-mkrescue bulunamadı (brew install i686-elf-grub xorriso mtools)"; exit 1; }
	mkdir -p build/isodir/boot/grub
	cp $(KERNEL) build/isodir/boot/rustos.bin
	cp grub.cfg build/isodir/boot/grub/grub.cfg
	$(GRUB_MKRESCUE) -o build/rustos.iso build/isodir
	@echo "Hazır: build/rustos.iso  (VirtualBox/VMware/QEMU veya gerçek donanıma yazılabilir)"

## run-iso: üretilen ISO'yu gerçek GRUB ile (—kernel OLMADAN) QEMU'da başlatır
run-iso: iso
	$(QEMU) $(DISPLAY_OPTS) $(VGA_OPTS) -cdrom build/rustos.iso $(DISK_OPTS)

# --- Limine ile UEFI + BIOS hibrit ISO (gerçek modern donanım için) ---
LIMINE_SHARE := /opt/homebrew/share/limine
OVMF         := /opt/homebrew/share/qemu/edk2-x86_64-code.fd
UEFI_ISO     := build/rustos-uefi.iso

## iso-uefi: hem UEFI hem BIOS ile açılan hibrit bir .iso üretir (Limine)
iso-uefi: build
	@test -f "$(LIMINE_SHARE)/BOOTX64.EFI" || { echo "HATA: Limine bulunamadı (brew install limine)"; exit 1; }
	rm -rf build/limine-iso
	mkdir -p build/limine-iso/boot/limine build/limine-iso/EFI/BOOT
	cp $(KERNEL) build/limine-iso/boot/rustos.bin
	cp limine.conf build/limine-iso/boot/limine/limine.conf
	cp $(LIMINE_SHARE)/limine-bios.sys $(LIMINE_SHARE)/limine-bios-cd.bin $(LIMINE_SHARE)/limine-uefi-cd.bin build/limine-iso/boot/limine/
	cp $(LIMINE_SHARE)/BOOTX64.EFI $(LIMINE_SHARE)/BOOTIA32.EFI build/limine-iso/EFI/BOOT/
	xorriso -as mkisofs -R -r -J \
		-b boot/limine/limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot boot/limine/limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		build/limine-iso -o $(UEFI_ISO)
	limine bios-install $(UEFI_ISO)
	@echo "Hazır: $(UEFI_ISO)  (UEFI + BIOS; USB'ye yazılabilir, modern donanımda açılır)"

## run-uefi: hibrit ISO'yu GERÇEK UEFI firmware (OVMF) ile QEMU'da test eder
OVMF_VARS_TPL := /opt/homebrew/share/qemu/edk2-i386-vars.fd
run-uefi: iso-uefi $(DISK)
	@mkdir -p build
	@test -f build/ovmf-vars.fd || cp $(OVMF_VARS_TPL) build/ovmf-vars.fd
	qemu-system-x86_64 -machine q35 -m 256M \
		-drive if=pflash,format=raw,unit=0,readonly=on,file=$(OVMF) \
		-drive if=pflash,format=raw,unit=1,file=build/ovmf-vars.fd \
		-cdrom $(UEFI_ISO) \
		-drive file=$(DISK),format=raw,if=ide \
		$(DISPLAY_OPTS) $(VGA_OPTS)

## run-bios-limine: aynı hibrit ISO'yu Legacy BIOS ile QEMU'da test eder
run-bios-limine: iso-uefi $(DISK)
	$(QEMU) $(DISPLAY_OPTS) $(VGA_OPTS) -cdrom $(UEFI_ISO) $(DISK_OPTS)

## clean: derleme çıktılarını siler (disk imajı korunur)
clean:
	$(CARGO) clean
	rm -rf build

## clean-disk: disk imajını da siler (kalıcı dosyalar kaybolur)
clean-disk:
	rm -f $(DISK)
