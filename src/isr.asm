; isr.asm — CPU istisnaları (0..31) için kesme stub'ları.
;
; x86'da istisna olduğunda CPU yığına EFLAGS, CS, EIP iter (ayrıcalık değişimi
; varsa ek olarak SS, ESP). Bazı istisnalar ek bir "hata kodu" (error code) da
; iter, bazıları itmez. Tek tip bir yığın çerçevesi (frame) elde etmek için:
;   - hata kodu OLMAYAN istisnalarda kukla bir 0 iteriz,
;   - sonra istisna numarasını iteriz,
;   - ortak koda (isr_common) atlarız.
; Ortak kod tüm yazmaçları kaydeder, Rust işleyicisini çağırır, geri yükler ve
; iret ile döner.
bits 32
section .text
extern isr_common_handler

; Hata kodu OLMAYAN istisna: kukla 0 + numara it.
%macro ISR_NOERR 1
global isr_stub_%1
isr_stub_%1:
    push dword 0
    push dword %1
    jmp isr_common
%endmacro

; Hata kodu OLAN istisna: CPU zaten hata kodunu itti; yalnızca numarayı it.
%macro ISR_ERR 1
global isr_stub_%1
isr_stub_%1:
    push dword %1
    jmp isr_common
%endmacro

ISR_NOERR 0    ; #DE  Bölme hatası
ISR_NOERR 1    ; #DB  Debug
ISR_NOERR 2    ;      NMI
ISR_NOERR 3    ; #BP  Breakpoint
ISR_NOERR 4    ; #OF  Overflow
ISR_NOERR 5    ; #BR  BOUND aşımı
ISR_NOERR 6    ; #UD  Geçersiz opcode
ISR_NOERR 7    ; #NM  Aygıt yok (FPU)
ISR_ERR   8    ; #DF  Çift hata (error code = 0)
ISR_NOERR 9    ;      Yardımcı işlemci segman aşımı (eski)
ISR_ERR   10   ; #TS  Geçersiz TSS
ISR_ERR   11   ; #NP  Segman yok
ISR_ERR   12   ; #SS  Yığın segman hatası
ISR_ERR   13   ; #GP  Genel koruma hatası
ISR_ERR   14   ; #PF  Sayfa hatası (page fault)
ISR_NOERR 15   ;      Ayrılmış
ISR_NOERR 16   ; #MF  x87 FPU hatası
ISR_ERR   17   ; #AC  Hizalama kontrolü
ISR_NOERR 18   ; #MC  Makine kontrolü
ISR_NOERR 19   ; #XM  SIMD kayan nokta
ISR_NOERR 20   ; #VE  Sanallaştırma
ISR_ERR   21   ; #CP  Kontrol koruması
ISR_NOERR 22
ISR_NOERR 23
ISR_NOERR 24
ISR_NOERR 25
ISR_NOERR 26
ISR_NOERR 27
ISR_NOERR 28
ISR_NOERR 29
ISR_NOERR 30
ISR_NOERR 31

isr_common:
    pusha               ; eax,ecx,edx,ebx,esp,ebp,esi,edi
    push ds
    push es
    push fs
    push gs
    ; Çekirdek veri segmentini yükle (ring3'ten gelmiş olabiliriz).
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov eax, esp        ; çerçeveye işaretçi (en üstte gs)
    push eax            ; Rust işleyicisine argüman (cdecl)
    call isr_common_handler
    add esp, 4          ; argümanı temizle
    pop gs
    pop fs
    pop es
    pop ds
    popa
    add esp, 8          ; istisna numarası + hata kodunu at
    iret

; --- Sistem çağrısı (int 0x80) ---
; Ring 3'ten gelir. CPU, TSS.ESP0 ile çekirdek yığınına geçer. Rust dağıtıcısı
; (syscall_dispatch) bir değer döndürür: 0 = ring3'e iret ile dön, 1 = çık
; (çekirdek bağlamına geri dön — enter_user_mode'un kaydettiği yığına).
extern syscall_dispatch
global isr_syscall
isr_syscall:
    pusha
    push ds
    push es
    push fs
    push gs
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov eax, esp
    push eax
    call syscall_dispatch
    add esp, 4
    test eax, eax
    jnz .do_exit
    pop gs
    pop fs
    pop es
    pop ds
    popa
    iret
.do_exit:
    mov esp, [kernel_resume_esp]
    popf
    popa
    ret

; --- Ring 3'e geçiş ---
; void enter_user_mode(u32 entry, u32 user_esp)
; Çekirdek bağlamını kaydeder ve iret ile ring 3'e (CPL=3) atlar. SYS_EXIT
; sistem çağrısı, kaydedilen bağlama geri döner (yukarıdaki .do_exit).
global enter_user_mode
enter_user_mode:
    pusha
    pushf
    mov [kernel_resume_esp], esp   ; geri dönüş noktası
    mov eax, [esp + 36 + 4]        ; entry  (pusha=32 + pushf=4 = 36)
    mov ecx, [esp + 36 + 8]        ; user_esp
    mov bx, 0x23                   ; kullanıcı veri seçicisi (DPL3)
    mov ds, bx
    mov es, bx
    mov fs, bx
    mov gs, bx
    push 0x23                      ; SS  = kullanıcı veri
    push ecx                       ; ESP = kullanıcı yığını
    push 0x202                     ; EFLAGS (IF=1: donanım kesmeleri açık)
    push 0x1B                      ; CS  = kullanıcı kod (DPL3)
    push eax                       ; EIP = giriş noktası
    iret

; --- Bağlam değiştirme (context switch) ---
; void context_switch(u32* old_esp, u32 new_esp)
; Çağıranın korunması gereken (callee-saved) yazmaçlarını yığına iter, geçerli
; ESP'yi *old_esp'e kaydeder, new_esp yığınına geçer ve oradan yazmaçları geri
; yükleyip döner. Böylece bir görevden diğerine geçilir (işbirlikçi zamanlama).
global context_switch
context_switch:
    push ebx
    push esi
    push edi
    push ebp
    mov eax, [esp + 20]   ; arg1: old_esp (4 yazmaç*4 + ret 4 = 20)
    mov [eax], esp        ; geçerli ESP'yi kaydet
    mov eax, [esp + 24]   ; arg2: new_esp
    mov esp, eax          ; yeni göreve geç
    pop ebp
    pop edi
    pop esi
    pop ebx
    ret

; void resume_user_mode(u32 entry, u32 user_esp, u32 eflags)
; Kesme bağlamından veya çekirdekten ring 3'e atlar (IF genelde açık).
global resume_user_mode
resume_user_mode:
    mov eax, [esp + 4]
    mov ecx, [esp + 8]
    mov edx, [esp + 12]
    mov bx, 0x23
    mov ds, bx
    mov es, bx
    mov fs, bx
    mov gs, bx
    push 0x23
    push ecx
    push edx
    push 0x1B
    push eax
    iret

; --- Donanım IRQ stub'ları (PIC yeniden eşlemesi sonrası 0x20..0x2F) ---
; IRQ0 = PIT zamanlayıcısı. Rust işleyicisine yığın çerçevesi (iret dahil) verilir.
extern irq0_handler
global irq0_stub
irq0_stub:
    pusha
    push ds
    push es
    push fs
    push gs
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov eax, esp
    push eax
    call irq0_handler
    add esp, 4
    pop gs
    pop fs
    pop es
    pop ds
    popa
    iret

; IRQ1 = PS/2 klavye
extern irq1_handler
global irq1_stub
irq1_stub:
    pusha
    push ds
    push es
    push fs
    push gs
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    call irq1_handler
    pop gs
    pop fs
    pop es
    pop ds
    popa
    iret

; Diğer IRQ'lar (beklenmeyen/spurious): yalnızca EOI gönderip yok say.
extern irq_generic_handler
global irq_generic_stub
irq_generic_stub:
    pusha
    push ds
    push es
    push fs
    push gs
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    call irq_generic_handler
    pop gs
    pop fs
    pop es
    pop ds
    popa
    iret

; Stub adreslerinin tablosu — Rust (idt.rs) IDT'yi bununla doldurur.
section .data
global kernel_resume_esp
kernel_resume_esp: dd 0

global isr_stub_table
isr_stub_table:
%assign i 0
%rep 32
    dd isr_stub_ %+ i
%assign i i+1
%endrep
