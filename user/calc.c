/* calc.c — MinOS için etkileşimli tamsayı hesap makinesi (ring 3 C uygulaması).
 *
 * + - * / ve parantez destekler; özyinelemeli inişli (recursive descent) ayrıştırıcı.
 * mini-libc'in printf/getline_/ctype'ını kullanır. Diskten `run calc` ile çalışır.
 */
#include "minos.h"
#include "stdio.h"
#include "stdlib.h"
#include "string.h"
#include "ctype.h"

static const char *p; /* ayrıştırma imleci */

static long expr(void);

static void skip_ws(void) {
    while (isspace((int)*p)) p++;
}

/* sayı | '(' ifade ')' | tekli eksi */
static long factor(void) {
    skip_ws();
    if (*p == '(') {
        p++;
        long v = expr();
        skip_ws();
        if (*p == ')') p++;
        return v;
    }
    if (*p == '-') {
        p++;
        return -factor();
    }
    if (*p == '+') {
        p++;
        return factor();
    }
    long v = 0;
    while (isdigit((int)*p)) {
        v = v * 10 + (*p - '0');
        p++;
    }
    return v;
}

/* çarpma / bölme */
static long term(void) {
    long v = factor();
    for (;;) {
        skip_ws();
        if (*p == '*') {
            p++;
            v *= factor();
        } else if (*p == '/') {
            p++;
            long d = factor();
            v = (d != 0) ? v / d : 0;
        } else {
            break;
        }
    }
    return v;
}

/* toplama / çıkarma */
static long expr(void) {
    long v = term();
    for (;;) {
        skip_ws();
        if (*p == '+') {
            p++;
            v += term();
        } else if (*p == '-') {
            p++;
            v -= term();
        } else {
            break;
        }
    }
    return v;
}

int main(void) {
    printf("=== MinOS Hesap Makinesi (tamsayi) ===\n");
    printf("Ifade gir: + - * / ve parantez. Cikis: 'q' veya bos satir.\n");

    char line[128];
    for (;;) {
        printf("hesap> ");
        int n = getline_(line, sizeof(line) - 1);
        if (n <= 0) break;
        line[n] = '\0';
        if (line[0] == 'q' && line[1] == '\0') break;
        p = line;
        long r = expr();
        printf("= %ld\n", r);
    }
    printf("Hesap makinesi kapandi.\n");
    return 0;
}
