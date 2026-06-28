/* Çekirdeğin bellek yerleşimini belirler.
 * Giriş noktası boot.asm içindeki _start etiketidir. */
ENTRY(_start)

SECTIONS {
    /* Çekirdek geleneksel olarak 1 MiB'den başlar. İlk 1 MiB'de
     * BIOS/donanım için ayrılmış bölgeler bulunur. */
    . = 1M;

    /* Multiboot başlığı dosyanın ilk 8 KiB'ı içinde olmalı,
     * bu yüzden en başa yerleştiriyoruz. */
    .boot : {
        KEEP(*(.multiboot_header))
    }

    .text : {
        *(.text .text.*)
    }

    .rodata : {
        *(.rodata .rodata.*)
    }

    .data : {
        *(.data .data.*)
    }

    .bss : {
        *(COMMON)
        *(.bss .bss.*)
    }

    /* Çekirdek imajının bittiği adres. Sayfalama/heap kodu (src/mem.rs)
     * boş RAM'i buradan itibaren kullanır. */
    . = ALIGN(4096);
    kernel_end = .;
}
