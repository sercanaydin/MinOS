; userprog.asm — Çekirdekten BAĞIMSIZ bir kullanıcı (ring 3) programı.
;
; Çekirdeğin parçası DEĞİLDİR; ayrı bir ELF olarak derlenip linklenir
; (build.rs, rust-lld ile) ve çalışma anında ELF yükleyici (src/elf.rs)
; tarafından kullanıcı adres alanına yüklenir. MinOS sistem çağrıları (int 0x80),
; Unix benzeri sözleşme — ebx/ecx/edx argüman, eax dönüş (-1 = hata):
;   eax=1  write(ebx=fd, ecx=tampon, edx=uzunluk)  -> yazılan bayt
;   eax=2  exit(ebx=kod)
;   eax=3  read(ebx=fd, ecx=tampon, edx=uzunluk)   -> okunan bayt
;   eax=4  getpid()                                -> PID
;   eax=5  open(ebx=ad, ecx=ad_uzunluk, edx=bayrak) -> fd  (0=oku,1=yaz)
;   eax=6  close(ebx=fd)
; fd 0 = klavye (stdin), fd 1 = konsol (stdout), fd >= 3 = dosya.
;
; Not: int 0x80 sonrası eax DIŞINDAKİ tüm registerlar korunur (stub pusha/popa).
bits 32

section .text
global _start
_start:
    ; --- getpid() ile süreç kimliğini al ve yaz ---
    mov eax, 4
    int 0x80
    mov esi, eax            ; esi = PID

    mov eax, 1
    mov ebx, 1
    mov ecx, pidmsg
    mov edx, pidmsg_len
    int 0x80

    mov eax, esi
    call print_uint

    mov eax, 1
    mov ebx, 1
    mov ecx, nl
    mov edx, 1
    int 0x80

    ; --- Kullanıcıdan isim oku (stdin) ---
    mov eax, 1
    mov ebx, 1
    mov ecx, prompt
    mov edx, prompt_len
    int 0x80

    mov eax, 3              ; read(0, buf, 64)
    mov ebx, 0
    mov ecx, buf
    mov edx, 64
    int 0x80
    mov esi, eax            ; esi = okunan uzunluk

    ; --- isim.txt dosyasına yaz ---
    mov eax, 5              ; open("isim.txt", O_WRONLY)
    mov ebx, fname
    mov ecx, fname_len
    mov edx, 1
    int 0x80
    mov edi, eax            ; edi = fd

    mov eax, 1              ; write(fd, buf, esi)
    mov ebx, edi
    mov ecx, buf
    mov edx, esi
    int 0x80

    mov eax, 6              ; close(fd)
    mov ebx, edi
    int 0x80

    ; --- isim.txt dosyasını tekrar açıp geri oku ---
    mov eax, 5              ; open("isim.txt", O_RDONLY)
    mov ebx, fname
    mov ecx, fname_len
    mov edx, 0
    int 0x80
    mov edi, eax            ; edi = fd

    mov eax, 3              ; read(fd, rbuf, 64)
    mov ebx, edi
    mov ecx, rbuf
    mov edx, 64
    int 0x80
    mov esi, eax            ; esi = okunan bayt

    mov eax, 6              ; close(fd)
    mov ebx, edi
    int 0x80

    ; --- Geri okunan içeriği yaz ---
    mov eax, 1
    mov ebx, 1
    mov ecx, filemsg
    mov edx, filemsg_len
    int 0x80

    mov eax, 1
    mov ebx, 1
    mov ecx, rbuf
    mov edx, esi
    int 0x80

    ; --- exit(0) ---
    mov eax, 2
    mov ebx, 0
    int 0x80
.hang:
    jmp .hang

; print_uint: eax içindeki işaretsiz sayıyı ondalık olarak stdout'a yazar.
print_uint:
    mov edi, numbuf_end
.next:
    xor edx, edx
    mov ecx, 10
    div ecx                 ; eax = eax/10, edx = kalan
    add dl, '0'
    dec edi
    mov [edi], dl
    test eax, eax
    jnz .next
    mov ecx, edi            ; ptr
    mov edx, numbuf_end
    sub edx, ecx            ; len
    mov ebx, 1              ; fd = stdout
    mov eax, 1
    int 0x80
    ret

section .data
pidmsg:      db "Surecin PID degeri: "
pidmsg_len   equ $ - pidmsg
prompt:      db "Adiniz nedir? "
prompt_len   equ $ - prompt
filemsg:     db "Diskteki isim.txt'ten geri okundu: "
filemsg_len  equ $ - filemsg
fname:       db "isim.txt"
fname_len    equ $ - fname
nl:          db 0x0A

section .bss
numbuf:      resb 12
numbuf_end:
buf:         resb 64
rbuf:        resb 64
