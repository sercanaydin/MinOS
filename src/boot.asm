; boot.asm — Multiboot v1 başlığı ve çekirdeğin giriş noktası.
; NASM ile elf32 olarak derlenir (build.rs içinde otomatik yapılır).
bits 32

; --- Multiboot v1 başlığı ---
; GRUB (ve QEMU'nun -kernel modu) bu başlığı dosyanın başında arar.
MBALIGN  equ 1 << 0            ; modülleri sayfa sınırına hizala
MEMINFO  equ 1 << 1            ; bellek haritası bilgisini iste
VIDEO    equ 1 << 2            ; grafik (framebuffer) modu iste
MBFLAGS  equ MBALIGN | MEMINFO | VIDEO
MAGIC    equ 0x1BADB002        ; Multiboot v1 sihirli sayısı
CHECKSUM equ -(MAGIC + MBFLAGS)

section .multiboot_header
align 4
    dd MAGIC
    dd MBFLAGS
    dd CHECKSUM
    ; VIDEO biti ayarlı: adres alanları kullanılmaz ama video alanları
    ; doğru ofsete (32) düşsün diye 5 boş dword bırakırız.
    dd 0                       ; header_addr   (kullanılmıyor)
    dd 0                       ; load_addr
    dd 0                       ; load_end_addr
    dd 0                       ; bss_end_addr
    dd 0                       ; entry_addr
    ; İstenen video modu: doğrusal grafik framebuffer.
    dd 0                       ; mode_type = 0 (doğrusal grafik)
    dd 1024                    ; tercih edilen genişlik  (piksel)
    dd 768                     ; tercih edilen yükseklik (piksel)
    dd 32                      ; tercih edilen renk derinliği (bit/piksel)

; --- Çekirdek yığını (stack) ---
; x86'da yığın aşağı doğru büyür; 16 KiB ayırıyoruz.
section .bss
align 16
stack_bottom:
    resb 16384
stack_top:

; --- Giriş noktası ---
section .text
global _start
extern kernel_main
_start:
    ; Yığın işaretçisini ayarla.
    mov esp, stack_top

    ; Multiboot bilgilerini Rust'a argüman olarak ilet:
    ;   eax = sihirli sayı, ebx = multiboot info yapısının adresi.
    push ebx
    push eax
    call kernel_main

    ; kernel_main asla dönmemeli; yine de döndüyse CPU'yu durdur.
.hang:
    cli
    hlt
    jmp .hang
