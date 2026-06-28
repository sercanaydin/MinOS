/* libminos.c — MinOS için minik bir C kütüphanesi (newlib-lite).
 *
 * Tam newlib/glibc DEĞİLDİR; eğitim amaçlı, gerçek newlib portuna giden yolda bir
 * köprü. Alt seviye G/Ç, newlib porting katmanı `syscalls.c` üzerinden yapılır
 * (_write/_read/_sbrk). Sağlananlar:
 *   - string.h : strlen/strcpy/strncpy/strcat/strncat/strcmp/strncmp/strchr/
 *                strrchr/memcpy/memmove/memset/memcmp
 *   - stdlib.h : malloc/calloc/realloc/free (sbrk tabanlı), atoi/strtol/abs/qsort
 *   - stdio.h  : putchar/puts/printf/snprintf/vsnprintf/getline_
 *
 * Not: clang, dizi/yapı kopya ve sıfırlamaları için memcpy/memset çağrısı
 * üretebilir; bunları burada sağladığımız için ayrıca bir libc'ye gerek yoktur.
 */
#include "minos.h"
#include "stdio.h"
#include "stdlib.h"
#include "string.h"

/* newlib porting katmanı (syscalls.c) */
extern void *_sbrk(int incr);
extern int _write(int fd, const void *buf, int len);
extern int _read(int fd, void *buf, int len);

/* ---------------- string.h ---------------- */

size_t strlen(const char *s) {
    size_t n = 0;
    while (s[n]) n++;
    return n;
}

char *strcpy(char *dst, const char *src) {
    char *d = dst;
    while ((*d++ = *src++)) {
    }
    return dst;
}

char *strncpy(char *dst, const char *src, size_t n) {
    size_t i = 0;
    for (; i < n && src[i]; i++) dst[i] = src[i];
    for (; i < n; i++) dst[i] = '\0';
    return dst;
}

char *strcat(char *dst, const char *src) {
    strcpy(dst + strlen(dst), src);
    return dst;
}

char *strncat(char *dst, const char *src, size_t n) {
    char *d = dst + strlen(dst);
    size_t i = 0;
    for (; i < n && src[i]; i++) d[i] = src[i];
    d[i] = '\0';
    return dst;
}

int strcmp(const char *a, const char *b) {
    while (*a && (*a == *b)) {
        a++;
        b++;
    }
    return (int)(unsigned char)*a - (int)(unsigned char)*b;
}

int strncmp(const char *a, const char *b, size_t n) {
    while (n && *a && (*a == *b)) {
        a++;
        b++;
        n--;
    }
    if (n == 0) return 0;
    return (int)(unsigned char)*a - (int)(unsigned char)*b;
}

char *strchr(const char *s, int c) {
    for (; *s; s++)
        if (*s == (char)c) return (char *)s;
    return (c == 0) ? (char *)s : 0;
}

char *strrchr(const char *s, int c) {
    const char *last = 0;
    for (; *s; s++)
        if (*s == (char)c) last = s;
    if (c == 0) return (char *)s;
    return (char *)last;
}

void *memcpy(void *dst, const void *src, size_t n) {
    unsigned char *d = (unsigned char *)dst;
    const unsigned char *s = (const unsigned char *)src;
    while (n--) *d++ = *s++;
    return dst;
}

void *memmove(void *dst, const void *src, size_t n) {
    unsigned char *d = (unsigned char *)dst;
    const unsigned char *s = (const unsigned char *)src;
    if (d == s || n == 0) return dst;
    if (d < s) {
        while (n--) *d++ = *s++;
    } else {
        d += n;
        s += n;
        while (n--) *--d = *--s;
    }
    return dst;
}

void *memset(void *dst, int c, size_t n) {
    unsigned char *d = (unsigned char *)dst;
    while (n--) *d++ = (unsigned char)c;
    return dst;
}

int memcmp(const void *a, const void *b, size_t n) {
    const unsigned char *x = (const unsigned char *)a;
    const unsigned char *y = (const unsigned char *)b;
    while (n--) {
        if (*x != *y) return (int)*x - (int)*y;
        x++;
        y++;
    }
    return 0;
}

/* ---------------- stdlib.h: malloc ailesi ---------------- */

/* sbrk tabanlı first-fit ayırıcı (K&R/OSdev tarzı). Heap, çekirdeğin SYS_SBRK
 * çağrısıyla talep üzerine büyür. Her blok başlık (boyut/free/next) + yük tutar. */
typedef struct block {
    size_t size; /* yük (payload) boyutu */
    int free;
    struct block *next;
} block_t;

#define BLOCK_HDR (sizeof(block_t))
static block_t *g_head = 0;
static block_t *g_tail = 0;

static block_t *grow_heap(size_t n) {
    block_t *b = (block_t *)_sbrk((int)(BLOCK_HDR + n));
    if (b == (void *)-1) return 0;
    b->size = n;
    b->free = 0;
    b->next = 0;
    if (g_tail) g_tail->next = b;
    g_tail = b;
    if (!g_head) g_head = b;
    return b;
}

void *malloc(size_t n) {
    if (n == 0) return 0;
    n = (n + 7u) & ~7u; /* 8 bayta hizala */

    for (block_t *b = g_head; b; b = b->next) {
        if (b->free && b->size >= n) {
            if (b->size >= n + BLOCK_HDR + 8) { /* yeterince büyükse böl */
                block_t *nb = (block_t *)((unsigned char *)b + BLOCK_HDR + n);
                nb->size = b->size - n - BLOCK_HDR;
                nb->free = 1;
                nb->next = b->next;
                b->next = nb;
                if (g_tail == b) g_tail = nb;
                b->size = n;
            }
            b->free = 0;
            return (unsigned char *)b + BLOCK_HDR;
        }
    }

    block_t *b = grow_heap(n);
    if (!b) return 0;
    return (unsigned char *)b + BLOCK_HDR;
}

void free(void *p) {
    if (!p) return;
    block_t *b = (block_t *)((unsigned char *)p - BLOCK_HDR);
    b->free = 1;
    if (b->next && b->next->free) { /* sonraki boşsa birleştir */
        if (g_tail == b->next) g_tail = b;
        b->size += BLOCK_HDR + b->next->size;
        b->next = b->next->next;
    }
}

void *calloc(size_t nmemb, size_t size) {
    size_t total = nmemb * size;
    void *p = malloc(total);
    if (p) memset(p, 0, total);
    return p;
}

void *realloc(void *p, size_t n) {
    if (!p) return malloc(n);
    if (n == 0) {
        free(p);
        return 0;
    }
    block_t *b = (block_t *)((unsigned char *)p - BLOCK_HDR);
    if (b->size >= n) return p; /* yeterli, yerinde kalsın */
    void *np = malloc(n);
    if (!np) return 0;
    memcpy(np, p, b->size);
    free(p);
    return np;
}

/* ---------------- stdlib.h: dönüşüm / yardımcı ---------------- */

int abs(int x) { return x < 0 ? -x : x; }

int atoi(const char *s) { return (int)strtol(s, 0, 10); }

long strtol(const char *s, char **end, int base) {
    long sign = 1, v = 0;
    while (*s == ' ' || *s == '\t' || *s == '\n') s++;
    if (*s == '-') {
        sign = -1;
        s++;
    } else if (*s == '+') {
        s++;
    }
    if ((base == 0 || base == 16) && s[0] == '0' && (s[1] == 'x' || s[1] == 'X')) {
        s += 2;
        base = 16;
    } else if (base == 0) {
        base = 10;
    }
    for (;;) {
        int c = *s, d;
        if (c >= '0' && c <= '9')
            d = c - '0';
        else if (c >= 'a' && c <= 'z')
            d = c - 'a' + 10;
        else if (c >= 'A' && c <= 'Z')
            d = c - 'A' + 10;
        else
            break;
        if (d >= base) break;
        v = v * base + d;
        s++;
    }
    if (end) *end = (char *)s;
    return sign * v;
}

/* Basit qsort (insertion sort — küçük diziler için yeterli, kararlı değil). */
void qsort(void *base, size_t n, size_t size,
           int (*cmp)(const void *, const void *)) {
    unsigned char *a = (unsigned char *)base;
    unsigned char tmp[256];
    if (size > sizeof(tmp)) return; /* öğe çok büyük: desteklenmiyor */
    for (size_t i = 1; i < n; i++) {
        memcpy(tmp, a + i * size, size);
        size_t j = i;
        while (j > 0 && cmp(a + (j - 1) * size, tmp) > 0) {
            memcpy(a + j * size, a + (j - 1) * size, size);
            j--;
        }
        memcpy(a + j * size, tmp, size);
    }
}

/* ---------------- stdio.h ---------------- */

/* Biçimlendirme çıktı bağlamı: ya stdout'a yazar ya da sınırlı bir tampona. */
typedef struct {
    char *buf;   /* tampon (snprintf), 0 ise stdout */
    size_t cap;  /* tampon kapasitesi */
    size_t len;  /* yazılan toplam (sınır gözetmeksizin sayaç) */
} out_t;

static void emit(out_t *o, char c) {
    if (o->buf) {
        if (o->len + 1 < o->cap) o->buf[o->len] = c;
    } else {
        _write(STDOUT, &c, 1);
    }
    o->len++;
}

static void emit_str(out_t *o, const char *s) {
    while (*s) emit(o, *s++);
}

static void emit_num(out_t *o, unsigned int v, int base, int is_signed, int upper) {
    char tmp[32];
    int i = 32;
    if (is_signed && (int)v < 0) {
        emit(o, '-');
        v = (unsigned int)(-(int)v);
    }
    const char *digs = upper ? "0123456789ABCDEF" : "0123456789abcdef";
    if (v == 0) tmp[--i] = '0';
    while (v) {
        tmp[--i] = digs[v % (unsigned int)base];
        v /= (unsigned int)base;
    }
    while (i < 32) emit(o, tmp[i++]);
}

static int do_format(out_t *o, const char *fmt, __builtin_va_list ap) {
    for (const char *p = fmt; *p; p++) {
        if (*p != '%') {
            emit(o, *p);
            continue;
        }
        p++;
        /* Uzunluk değiştiricilerini atla (l, ll, h, hh, z, j, t, L). i386'da
         * long = 32-bit olduğundan %ld/%lu, %d/%u ile aynı genişlikte okunur.
         * (long long = 64-bit istisnadır; desteklenmez.) */
        while (*p == 'l' || *p == 'h' || *p == 'z' || *p == 'j' || *p == 't' ||
               *p == 'L') {
            p++;
        }
        switch (*p) {
        case 'd':
        case 'i':
            emit_num(o, (unsigned int)__builtin_va_arg(ap, int), 10, 1, 0);
            break;
        case 'u':
            emit_num(o, __builtin_va_arg(ap, unsigned int), 10, 0, 0);
            break;
        case 'x':
            emit_num(o, __builtin_va_arg(ap, unsigned int), 16, 0, 0);
            break;
        case 'X':
            emit_num(o, __builtin_va_arg(ap, unsigned int), 16, 0, 1);
            break;
        case 'p':
            emit_str(o, "0x");
            emit_num(o, __builtin_va_arg(ap, unsigned int), 16, 0, 0);
            break;
        case 'c':
            emit(o, (char)__builtin_va_arg(ap, int));
            break;
        case 's': {
            char *s = __builtin_va_arg(ap, char *);
            emit_str(o, s ? s : "(null)");
            break;
        }
        case '%':
            emit(o, '%');
            break;
        case 0:
            p--;
            break;
        default:
            emit(o, '%');
            emit(o, *p);
            break;
        }
    }
    return (int)o->len;
}

int putchar(int c) {
    char ch = (char)c;
    _write(STDOUT, &ch, 1);
    return c;
}

int puts(const char *s) {
    _write(STDOUT, s, (int)strlen(s));
    putchar('\n');
    return 0;
}

int getline_(char *buf, int max) { return _read(STDIN, buf, max); }

int printf(const char *fmt, ...) {
    out_t o = {0, 0, 0};
    __builtin_va_list ap;
    __builtin_va_start(ap, fmt);
    int n = do_format(&o, fmt, ap);
    __builtin_va_end(ap);
    return n;
}

int vsnprintf(char *buf, size_t size, const char *fmt, __builtin_va_list ap) {
    out_t o = {buf, size, 0};
    int n = do_format(&o, fmt, ap);
    if (buf && size) buf[(o.len < size) ? o.len : size - 1] = '\0';
    return n;
}

int snprintf(char *buf, size_t size, const char *fmt, ...) {
    __builtin_va_list ap;
    __builtin_va_start(ap, fmt);
    int n = vsnprintf(buf, size, fmt, ap);
    __builtin_va_end(ap);
    return n;
}
