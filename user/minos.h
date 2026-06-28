/* minos.h — MinOS kullanıcı (ring 3) programları için minik C başlığı.
 *
 * libc YOK. Sadece çekirdeğin `int 0x80` sistem çağrıları ve birkaç küçük
 * yardımcı (strlen/puts/print_int). Tümü `static inline` olduğundan ayrı bir
 * kütüphane derlemeye gerek yoktur — sadece bu başlığı dahil et.
 *
 * Sistem çağrısı sözleşmesi (Unix benzeri): eax = numara, ebx/ecx/edx = arg,
 * dönüş eax. (Bkz. src/idt.rs::syscall_dispatch.)
 */
#ifndef MINOS_H
#define MINOS_H

#define SYS_WRITE 1
#define SYS_EXIT 2
#define SYS_READ 3
#define SYS_GETPID 4
#define SYS_OPEN 5
#define SYS_CLOSE 6
#define SYS_SLEEP 7
#define SYS_SBRK 8
#define SYS_FETCH 9
#define SYS_GMODE 10
#define SYS_FILLRECT 11
#define SYS_BLIT 12
#define SYS_GETARG 13

#define STDIN 0
#define STDOUT 1

#define O_RDONLY 0
#define O_WRONLY 1

static inline int __syscall(int n, int a, int b, int c) {
    int ret;
    __asm__ volatile("int $0x80"
                     : "=a"(ret)
                     : "a"(n), "b"(a), "c"(b), "d"(c)
                     : "memory");
    return ret;
}

static inline int write(int fd, const void *buf, int len) {
    return __syscall(SYS_WRITE, fd, (int)buf, len);
}
static inline int read(int fd, void *buf, int len) {
    return __syscall(SYS_READ, fd, (int)buf, len);
}
static inline void exit(int code) {
    __syscall(SYS_EXIT, code, 0, 0);
    for (;;) {
    }
}
static inline int getpid(void) { return __syscall(SYS_GETPID, 0, 0, 0); }
static inline int open(const char *name, int len, int flags) {
    return __syscall(SYS_OPEN, (int)name, len, flags);
}
static inline int close(int fd) { return __syscall(SYS_CLOSE, fd, 0, 0); }
static inline void sleep(int ticks) { __syscall(SYS_SLEEP, ticks, 0, 0); }

/* getarg(buf, cap): `run <prog> <arg...>` ile verilen argüman dizesini buf'a
 * (en çok cap bayt) kopyalar, uzunluğu döner. crt0 bunu argv'ye böler. */
static inline int getarg(char *buf, int cap) {
    return __syscall(SYS_GETARG, (int)buf, cap, 0);
}

/* sbrk(inc): program kesilme noktasını (heap tepesi) inc kadar değiştirir,
 * eski tepeyi döndürür. Hata durumunda (void*)-1 döner. malloc bunu kullanır. */
static inline void *sbrk(int inc) {
    return (void *)__syscall(SYS_SBRK, inc, 0, 0);
}

/* fetch(url, out, cap): çekirdeğin HTTP/HTTPS yığınıyla url'yi çeker, yanıt
 * gövdesini (başlıklar dahil) out'a (en çok cap bayt) yazar; bayt sayısını döner.
 * Hata: -1. URL "http://..." veya "https://..." biçiminde C-dizisi olmalı. */
static inline int fetch(const char *url, void *out, int cap) {
    return __syscall(SYS_FETCH, (int)url, (int)out, cap);
}

/* --- Grafik (framebuffer) --- */

/* gmode(): grafik moda geçer; (genişlik<<16)|yükseklik döner, 0 = grafik yok. */
static inline int gmode(void) { return __syscall(SYS_GMODE, 0, 0, 0); }
#define GMODE_W(r) ((r) >> 16)
#define GMODE_H(r) ((r) & 0xFFFF)

/* fill_rect: dolu dikdörtgen (renk = 0xRRGGBB). */
static inline void fill_rect(int x, int y, int w, int h, int rgb) {
    __syscall(SYS_FILLRECT, ((x & 0xFFFF) << 16) | (y & 0xFFFF),
              ((w & 0xFFFF) << 16) | (h & 0xFFFF), rgb);
}

/* blit: kullanıcı 0xRRGGBB tamponunu (w×h) ekrana (x,y)'ye basar. */
static inline int blit(const void *buf, int x, int y, int w, int h) {
    return __syscall(SYS_BLIT, (int)buf, ((x & 0xFFFF) << 16) | (y & 0xFFFF),
                     ((w & 0xFFFF) << 16) | (h & 0xFFFF));
}

#endif /* MINOS_H */
