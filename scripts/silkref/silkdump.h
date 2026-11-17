/* shravan reference dumps: one text record per line, appended to $SILKDUMP. */
#ifndef SILKDUMP_H
#define SILKDUMP_H
#include <stdio.h>
#include <stdlib.h>
static FILE *sd_open(void) {
    static FILE *f = 0; static int tried = 0;
    if (!tried) { const char *p = getenv("SILKDUMP"); tried = 1; if (p) { f = fopen(p, "a"); if (f) setvbuf(f, 0, _IOLBF, 0); } }
    return f;
}
static void sd_f(const char *t, const float *v, int n) { FILE *f = sd_open(); int i; if (!f) return; fprintf(f, "%s", t); for (i = 0; i < n; i++) fprintf(f, " %.9g", (double)v[i]); fprintf(f, "\n"); fflush(f); }
static void sd_d(const char *t, const double *v, int n) { FILE *f = sd_open(); int i; if (!f) return; fprintf(f, "%s", t); for (i = 0; i < n; i++) fprintf(f, " %.17g", v[i]); fprintf(f, "\n"); fflush(f); }
static void sd_i16(const char *t, const short *v, int n) { FILE *f = sd_open(); int i; if (!f) return; fprintf(f, "%s", t); for (i = 0; i < n; i++) fprintf(f, " %d", v[i]); fprintf(f, "\n"); fflush(f); }
static void sd_i32(const char *t, const int *v, int n) { FILE *f = sd_open(); int i; if (!f) return; fprintf(f, "%s", t); for (i = 0; i < n; i++) fprintf(f, " %d", v[i]); fprintf(f, "\n"); fflush(f); }
static void sd_i8(const char *t, const signed char *v, int n) { FILE *f = sd_open(); int i; if (!f) return; fprintf(f, "%s", t); for (i = 0; i < n; i++) fprintf(f, " %d", v[i]); fprintf(f, "\n"); fflush(f); }
static void sd_int(const char *t, int v) { sd_i32(t, &v, 1); }
static void sd_flt(const char *t, float v) { sd_f(t, &v, 1); }
#endif
