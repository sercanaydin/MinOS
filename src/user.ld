/* Kullanıcı (ring 3) programının bellek yerleşimi.
 * Çekirdeğin sayfalama kodu kullanıcı bölgesini 0x4000_0000'dan (1 GiB)
 * başlatır; program da oraya linklenir. Segmentler sayfa (4 KiB) hizalı
 * olsun ki ELF yükleyici her birini ayrı sayfalara temiz biçimde eşleyebilsin. */
ENTRY(_start)

SECTIONS {
    . = 0x40000000;

    .text : {
        *(.text .text.*)
    }

    . = ALIGN(0x1000);
    .rodata : {
        *(.rodata .rodata.*)
    }

    . = ALIGN(0x1000);
    .data : {
        *(.data .data.*)
    }

    . = ALIGN(0x1000);
    .bss : {
        *(COMMON)
        *(.bss .bss.*)
    }
}
