/* paint.c — MinOS için grafik demosu (ring 3 C uygulaması).
 *
 * SYS_GMODE ile grafik moda geçer; kendi malloc'ladığı tampona bir gradyan çizip
 * tek `blit` ile ekrana basar, üstüne `fill_rect` ile kutular ekler ve kısa bir
 * zıplayan kare animasyonu oynatır. Diskten `run paint` ile çalışır.
 */
#include "minos.h"
#include "stdio.h"
#include "stdlib.h"

int main(void) {
    int m = gmode();
    if (!m) {
        printf("grafik mod yok (VBE desteklenmiyor).\n");
        return 1;
    }
    int W = GMODE_W(m);
    int H = GMODE_H(m);
    printf("grafik: %dx%d\n", W, H);

    /* Kendi tamponumuza gradyan ciz, tek blit ile bas. */
    unsigned int *cv = (unsigned int *)malloc((unsigned)(W * H) * 4u);
    if (cv) {
        for (int y = 0; y < H; y++) {
            for (int x = 0; x < W; x++) {
                int r = x * 255 / W;
                int g = y * 255 / H;
                int b = 100;
                cv[y * W + x] = (unsigned)((r << 16) | (g << 8) | b);
            }
        }
        blit(cv, 0, 0, W, H);
        free(cv);
    }

    /* Ortaya bir pano + baslik kutusu */
    fill_rect(W / 2 - 160, H / 2 - 90, 320, 180, 0x202830);
    fill_rect(W / 2 - 150, H / 2 - 80, 300, 50, 0x00CC66);
    fill_rect(W / 2 - 150, H / 2 - 20, 300, 90, 0x101820);

    /* Zipliyan kare animasyonu (fill_rect + sleep). */
    int x = 40, y = 40, dx = 9, dy = 7, s = 36;
    for (int f = 0; f < 90; f++) {
        fill_rect(x, y, s, s, 0x0A1018); /* eski kareyi koyu renkle sil */
        x += dx;
        y += dy;
        if (x < 0 || x + s > W) {
            dx = -dx;
            x += dx;
        }
        if (y < 0 || y + s > H) {
            dy = -dy;
            y += dy;
        }
        fill_rect(x, y, s, s, 0xFFCC00); /* yeni kare */
        sleep(2);
    }

    printf("paint bitti (grafik moddasiniz).\n");
    return 0;
}
