/* stdio.h — MinOS mini-libc: temel konsol G/Ç (libminos.c'de).
 * Tam newlib değil; %d %i %u %x %X %p %c %s %% biçim belirteçlerini destekler. */
#ifndef _STDIO_H
#define _STDIO_H

#ifndef _SIZE_T_DEFINED
#define _SIZE_T_DEFINED
typedef unsigned int size_t;
#endif

int putchar(int c);
int puts(const char *s); /* standart: dizeyi + '\n' yazar */
int printf(const char *fmt, ...);
int snprintf(char *buf, size_t size, const char *fmt, ...);
int vsnprintf(char *buf, size_t size, const char *fmt, __builtin_va_list ap);
int getline_(char *buf, int max); /* satır okur (stdin), '\n' hariç uzunluk döner */

#endif /* _STDIO_H */
