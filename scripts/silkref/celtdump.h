/* shravan reference dumps for the CELT encoder: one text record per line,
 * appended to $CELTDUMP. Floats as their IEEE-754 single bit patterns in hex
 * (exact), integers in decimal. src/celt_enc.cyr built with -D CELT_TRACE
 * writes the same records, so the two compare line for line. */
#ifndef CELTDUMP_H
#define CELTDUMP_H
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static FILE *cd_open(void) {
    static FILE *f = 0; static int tried = 0;
    if (!tried) { const char *p = getenv("CELTDUMP"); tried = 1; if (p) f = fopen(p, "a"); }
    return f;
}
static void cd_f(const char *t, const float *v, int n) {
    FILE *f = cd_open(); int i; if (!f) return;
    fprintf(f, "%s", t);
    for (i = 0; i < n; i++) { unsigned int u; memcpy(&u, &v[i], 4); fprintf(f, " %08x", u); }
    fprintf(f, "\n");
}
static void cd_i(const char *t, const int *v, int n) {
    FILE *f = cd_open(); int i; if (!f) return;
    fprintf(f, "%s", t);
    for (i = 0; i < n; i++) fprintf(f, " %d", v[i]);
    fprintf(f, "\n");
}
static void cd_u8(const char *t, const unsigned char *v, int n) {
    FILE *f = cd_open(); int i; if (!f) return;
    fprintf(f, "%s", t);
    for (i = 0; i < n; i++) fprintf(f, " %d", v[i]);
    fprintf(f, "\n");
}
static void cd_int(const char *t, int v) { cd_i(t, &v, 1); }
static void cd_flt(const char *t, float v) { cd_f(t, &v, 1); }
#endif
