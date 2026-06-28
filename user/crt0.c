/* crt0.c — C kullanıcı programının giriş kodu (C runtime 0).
 *
 * ELF giriş noktası `_start` buradadır (user.ld: ENTRY(_start)). Çekirdek
 * programı ring 3'te `_start`'tan başlatır; biz de `main()`'i argc/argv ile
 * çağırıp dönüş değeriyle `exit()` yaparız.
 *
 * argv: çekirdek `run <prog> <arg...>` satırındaki argüman dizesini SYS_GETARG
 * ile verir; burada boşluklara göre bölüp argv[1..] olarak iletiriz. argv[0]
 * program adının yerine bir yer tutucudur. `int main(void)` yazan programlar
 * fazladan argümanları görmezden gelir (cdecl), sorun olmaz.
 */
#include "minos.h"

extern int main(int argc, char **argv);

#define ARG_CAP 256
#define ARGV_MAX 32

static char argbuf[ARG_CAP];
static char *argv[ARGV_MAX];

void _start(void) {
    int n = getarg(argbuf, ARG_CAP - 1);
    if (n < 0) n = 0;
    argbuf[n] = 0;

    int argc = 0;
    argv[argc++] = "prog"; /* argv[0] */

    int i = 0;
    while (argbuf[i] && argc < ARGV_MAX) {
        while (argbuf[i] == ' ') i++;
        if (!argbuf[i]) break;
        argv[argc++] = &argbuf[i];
        while (argbuf[i] && argbuf[i] != ' ') i++;
        if (argbuf[i] == ' ') {
            argbuf[i] = 0;
            i++;
        }
    }

    exit(main(argc, argv));
}
