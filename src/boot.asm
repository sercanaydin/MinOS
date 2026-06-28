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
; x86'da yığın aşağı doğru büyür. TLS/kripto işlemleri yığını zorladığından
; 64 KiB ayırıyoruz.
section .bss
align 16
stack_bottom:
    resb 65536
stack_top:

; --- Giriş noktası ---
section .text
global _start
extern kernel_main
_start:
    ; Yığın işaretçisini ayarla.
    mov esp, stack_top

    ; --- SSE/SSE2'yi etkinleştir ---
    ; Kripto kütüphaneleri (aes/polyval/sha2) tamsayı SIMD (SSE2) komutları
    ; üretir; bunlar CR0.EM=0 ve CR4.OSFXSR=1 olmadan #UD verir.
    ; ÖNEMLİ: Multiboot sihirli sayısı eax'te, bilgi yapısı ebx'te gelir;
    ; bunları ezmemek için yalnızca ecx kullanıyoruz.
    mov ecx, cr0
    and ecx, 0xFFFFFFF3      ; EM=0 (bit2), TS=0 (bit3) temizle
    or  ecx, 0x00000002      ; MP=1 (bit1)
    mov cr0, ecx
    fninit
    mov ecx, cr4
    or  ecx, 0x00000600      ; OSFXSR (bit9) + OSXMMEXCPT (bit10)
    mov cr4, ecx

    ; Multiboot bilgilerini Rust'a argüman olarak ilet:
    ;   eax = sihirli sayı, ebx = multiboot info yapısının adresi.
    ; SSE hizalı erişimler için yığını 16 bayta hizala (2 argüman = 8 bayt).
    sub esp, 8
    push ebx
    push eax
    call kernel_main

    ; kernel_main asla dönmemeli; yine de döndüyse CPU'yu durdur.
.hang:
    cli
    hlt
    jmp .hang
