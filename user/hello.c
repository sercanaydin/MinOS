/* hello.c — MinOS için örnek C programı: ring 3'te çalışan minik bir "tarayıcı".
 *
 * newlib-lite (printf/malloc/snprintf) + SYS_FETCH sistem çağrısını kullanır:
 * çekirdeğin HTTP/HTTPS (smoltcp + rustls) yığınını çağırıp bir web sayfasını
 * çeker ve ekrana basar. Yani ağ tamamen kullanıcı alanından sürülür.
 *
 * clang ile 32-bit freestanding ELF olarak derlenir, çekirdeğe gömülür,
 * `cprog` komutuyla çalıştırılır.
 */
#include "minos.h"
#include "stdio.h"
#include "stdlib.h"
#include "string.h"

int main(void) {
    printf("=== MinOS C tarayici (mini-libc + SYS_FETCH) ===\n");

    char info[72];
    snprintf(info, sizeof(info), "libc hazir: malloc/printf/snprintf, pid=%d", getpid());
    printf("%s\n\n", info);

    const char *url = "http://example.com/";
    int cap = 16 * 1024;
    char *body = (char *)malloc(cap);
    if (!body) {
        printf("malloc(%d) basarisiz\n", cap);
        return 1;
    }

    printf("GET %s\n", url);
    int n = fetch(url, body, cap - 1);
    if (n < 0) {
        printf("fetch hatasi (ag yapilandirilmamis olabilir).\n");
        free(body);
        return 1;
    }
    body[n] = '\0';

    printf("--- yanit: %d bayt (ilk 700 gosteriliyor) ---\n", n);
    int show = n < 700 ? n : 700;
    write(STDOUT, body, show);
    printf("\n--- son ---\n");

    free(body);
    return 0;
}
