/* stdlib.h — MinOS mini-libc: bellek ayırma + dönüşüm (libminos.c'de). */
#ifndef _STDLIB_H
#define _STDLIB_H

#ifndef _SIZE_T_DEFINED
#define _SIZE_T_DEFINED
typedef unsigned int size_t;
#endif

void *malloc(size_t n);
void *calloc(size_t nmemb, size_t size);
void *realloc(void *p, size_t n);
void free(void *p);

int atoi(const char *s);
long strtol(const char *s, char **end, int base);
int abs(int x);
void qsort(void *base, size_t n, size_t size,
           int (*cmp)(const void *, const void *));

#endif /* _STDLIB_H */
