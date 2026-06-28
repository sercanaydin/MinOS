/* syscalls.c — newlib porting (alt seviye sistem) katmanı.
 *
 * Bu dosya, bir libc'yi (newlib) yeni bir işletim sistemine taşırken yazılan
 * KANONİK katmandır: libc'nin beklediği `_sbrk`, `_write`, `_read`, `_open`,
 * `_close`, `_lseek`, `_fstat`, `_isatty`, `_exit`, `_getpid` çağrılarını MinOS'un
 * `int 0x80` sistem çağrılarına bağlar.
 *
 * Şu an üstünde kendi mini-libc'imiz (libminos.c) çalışıyor. İleride GERÇEK newlib
 * derlenip eklendiğinde, kendi stdio/stdlib'imizi atıp bu dosyayı (gerekirse
 * newlib'in <sys/stat.h> struct stat'ına uyarlayarak) olduğu gibi kullanacağız.
 *
 * Sözleşme: eax = numara, ebx/ecx/edx = arg, dönüş eax (-1 = hata).
 */
#include "minos.h"

static int sys3(int n, int a, int b, int c) { return __syscall(n, a, b, c); }

void *_sbrk(int incr) { return (void *)sys3(SYS_SBRK, incr, 0, 0); }

int _write(int fd, const void *buf, int len) {
    return sys3(SYS_WRITE, fd, (int)buf, len);
}

int _read(int fd, void *buf, int len) {
    return sys3(SYS_READ, fd, (int)buf, len);
}

int _open(const char *name, int len, int flags) {
    return sys3(SYS_OPEN, (int)name, len, flags);
}

int _close(int fd) { return sys3(SYS_CLOSE, fd, 0, 0); }

int _getpid(void) { return sys3(SYS_GETPID, 0, 0, 0); }

void _exit(int code) {
    sys3(SYS_EXIT, code, 0, 0);
    for (;;) {
    }
}

/* Çekirdek henüz seek/stat desteklemiyor — newlib'in ihtiyaç duyduğu stub'lar.
 * Gerçek newlib portunda RFS'ye lseek/fstat eklenince burası genişletilecek. */
int _lseek(int fd, int off, int whence) {
    (void)fd;
    (void)off;
    (void)whence;
    return -1;
}

int _isatty(int fd) { return (fd == STDIN || fd == STDOUT) ? 1 : 0; }

int _fstat(int fd, void *st) {
    (void)fd;
    (void)st;
    return -1;
}
