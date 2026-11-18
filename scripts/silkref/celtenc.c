/* celtenc: libopus's CELT encoder, celt_encode_with_ec, the reference for
 * shravan's src/celt_enc.cyr.
 *   celtenc <in.f64> <out.pk> <fs> <channels> [key=value ...]
 * in.f64: interleaved doubles, passed as floats (opus_res, the float build's
 * input). out.pk: per
 * frame, u32le length then the frame; out.pk.rng: per frame the final range
 * (u32le). Keys (the CELT encoder's ctls; defaults are celt_encoder_init's):
 *   n=N (samples a frame at fs; fs/50)  frames=n,n,... (each frame's in turn, cycled)
 *   bitrate=N|max  rates=N,N,... (cycled)  vbr=0|1  vbrc=0|1  cx=0..10
 *   loss=0..100  pred=0|1|2 (CELT_SET_PREDICTION)  lsb=8..24
 *   inv=0|1 (OPUS_SET_PHASE_INVERSION_DISABLED)  start=N  end=N  lfe=0|1
 *   stream=1|2 (CELT_SET_CHANNELS)  streams=1212... (cycled)
 *   bytes=N (nbCompressedBytes; 1275)  sizes=N,N,... (cycled)
 *   reset=N (OPUS_RESET_STATE before every Nth frame)  total=N (frames)
 *   ana=seed   a synthetic AnalysisInfo each frame (CELT_SET_ANALYSIS)
 *   silk=seed  a synthetic SILKInfo each frame (CELT_SET_SILK_INFO)
 *   mask=seed  a synthetic energy mask each frame (OPUS_SET_ENERGY_MASK)
 *   pre=seed   symbols written into the range coder before CELT's, as SILK's
 *              are in a hybrid frame (and 0 to 48 raw bits); CELT then encodes
 *              into that coder
 * The synthetic values come from a 32-bit LCG; src/celt_enc_tests.cyr draws
 * the same. Every float drawn is a multiple of 2^-24, exact in both. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "celt.h"
#include "entenc.h"
#include "opus_defines.h"

static unsigned int lcg(unsigned int *s) { *s = *s * 1664525u + 1013904223u; return *s; }
static float unit(unsigned int *s) { return (float)(lcg(s) >> 8) * (1.f / 16777216.f); }

static int list(const char *s, int *out, int max) {
    int n = 0;
    while (*s && n < max) {
        char *e;
        long v = strtol(s, &e, 10), c = 1;
        if (e == s) break;
        if (*e == '*') c = strtol(e + 1, &e, 10);
        while (c-- > 0 && n < max) out[n++] = (int)v;
        s = *e == ',' ? e + 1 : e;
    }
    return n;
}

int main(int argc, char **argv) {
    if (argc < 5) { fprintf(stderr, "usage: celtenc in.f64 out.pk fs channels [key=value ...]\n"); return 2; }
    int fs = atoi(argv[3]), ch = atoi(argv[4]);
    CELTEncoder *st = malloc(celt_encoder_get_size(ch));
    if (celt_encoder_init(st, fs, ch, 0) != OPUS_OK) { fprintf(stderr, "init failed\n"); return 1; }
    celt_encoder_ctl(st, CELT_SET_SIGNALLING(0));
    int nb = 1275, total = -1, reset = 0, n0 = fs / 50;
    unsigned int ana = 0, silk = 0, mask = 0, pre = 0;
    int use_ana = 0, use_silk = 0, use_mask = 0, use_pre = 0;
    static int frames[4096], rates[4096], sizes[4096];
    int nframes = 0, nrates = 0, nsizes = 0;
    const char *streams = NULL;
    for (int i = 5; i < argc; i++) {
        char *k = argv[i], *v = strchr(k, '=');
        if (!v) { fprintf(stderr, "bad option %s\n", k); return 2; }
        *v++ = 0;
        int r = OPUS_OK;
        if (!strcmp(k, "n")) n0 = atoi(v);
        else if (!strcmp(k, "frames")) nframes = list(v, frames, 4096);
        else if (!strcmp(k, "bitrate")) r = celt_encoder_ctl(st, OPUS_SET_BITRATE(!strcmp(v, "max") ? OPUS_BITRATE_MAX : atoi(v)));
        else if (!strcmp(k, "rates")) nrates = list(v, rates, 4096);
        else if (!strcmp(k, "vbr")) r = celt_encoder_ctl(st, OPUS_SET_VBR(atoi(v)));
        else if (!strcmp(k, "vbrc")) r = celt_encoder_ctl(st, OPUS_SET_VBR_CONSTRAINT(atoi(v)));
        else if (!strcmp(k, "cx")) r = celt_encoder_ctl(st, OPUS_SET_COMPLEXITY(atoi(v)));
        else if (!strcmp(k, "loss")) r = celt_encoder_ctl(st, OPUS_SET_PACKET_LOSS_PERC(atoi(v)));
        else if (!strcmp(k, "pred")) r = celt_encoder_ctl(st, CELT_SET_PREDICTION(atoi(v)));
        else if (!strcmp(k, "lsb")) r = celt_encoder_ctl(st, OPUS_SET_LSB_DEPTH(atoi(v)));
        else if (!strcmp(k, "inv")) r = celt_encoder_ctl(st, OPUS_SET_PHASE_INVERSION_DISABLED(atoi(v)));
        else if (!strcmp(k, "start")) r = celt_encoder_ctl(st, CELT_SET_START_BAND(atoi(v)));
        else if (!strcmp(k, "end")) r = celt_encoder_ctl(st, CELT_SET_END_BAND(atoi(v)));
        else if (!strcmp(k, "lfe")) r = celt_encoder_ctl(st, OPUS_SET_LFE(atoi(v)));
        else if (!strcmp(k, "stream")) r = celt_encoder_ctl(st, CELT_SET_CHANNELS(atoi(v)));
        else if (!strcmp(k, "streams")) streams = v;
        else if (!strcmp(k, "bytes")) nb = atoi(v);
        else if (!strcmp(k, "sizes")) nsizes = list(v, sizes, 4096);
        else if (!strcmp(k, "reset")) reset = atoi(v);
        else if (!strcmp(k, "total")) total = atoi(v);
        else if (!strcmp(k, "ana")) { ana = (unsigned int)strtoul(v, 0, 10); use_ana = 1; }
        else if (!strcmp(k, "silk")) { silk = (unsigned int)strtoul(v, 0, 10); use_silk = 1; }
        else if (!strcmp(k, "mask")) { mask = (unsigned int)strtoul(v, 0, 10); use_mask = 1; }
        else if (!strcmp(k, "pre")) { pre = (unsigned int)strtoul(v, 0, 10); use_pre = 1; }
        else { fprintf(stderr, "bad option %s\n", k); return 2; }
        if (r != OPUS_OK) { fprintf(stderr, "ctl %s=%s: %d\n", k, v, r); return 1; }
    }
    FILE *fi = fopen(argv[1], "rb"), *fo = fopen(argv[2], "wb");
    char rpath[4096];
    snprintf(rpath, sizeof rpath, "%s.rng", argv[2]);
    FILE *fr = fopen(rpath, "wb");
    if (!fi || !fo || !fr) { fprintf(stderr, "cannot open files\n"); return 2; }
    fseek(fi, 0, SEEK_END); long len = ftell(fi) / 8 / ch; fseek(fi, 0, SEEK_SET);
    double *d = malloc(len * ch * 8 + 16);
    if (fread(d, 8, len * ch, fi) != (size_t)(len * ch)) return 2;
    float *pcm = malloc(len * ch * 4 + 16);
    for (long i = 0; i < len * ch; i++) pcm[i] = (float)d[i];
    unsigned char *pkt = malloc(1276 * 2);
    static float energy_mask[2 * 21];
    long pos = 0;
    int count = 0;
    for (;;) {
        if (total >= 0 && count >= total) break;
        int n = nframes ? frames[count % nframes] : n0;
        if (pos + n > len) break;
        if (reset > 0 && count > 0 && count % reset == 0) celt_encoder_ctl(st, OPUS_RESET_STATE);
        if (nrates) celt_encoder_ctl(st, OPUS_SET_BITRATE(rates[count % nrates]));
        if (streams) celt_encoder_ctl(st, CELT_SET_CHANNELS(streams[count % strlen(streams)] - '0'));
        int bytes = nsizes ? sizes[count % nsizes] : nb;
        if (use_ana) {
            AnalysisInfo a;
            memset(&a, 0, sizeof a);
            a.valid = (lcg(&ana) >> 29) != 0;
            a.tonality = unit(&ana);
            a.tonality_slope = unit(&ana) - .5f;
            a.noisiness = unit(&ana);
            a.activity = unit(&ana);
            a.music_prob = unit(&ana);
            a.music_prob_min = unit(&ana);
            a.music_prob_max = unit(&ana);
            a.bandwidth = 1 + (int)(lcg(&ana) % 20);
            a.activity_probability = unit(&ana);
            a.max_pitch_ratio = unit(&ana);
            for (int i = 0; i < LEAK_BANDS; i++) a.leak_boost[i] = (unsigned char)(lcg(&ana) >> 24);
            celt_encoder_ctl(st, CELT_SET_ANALYSIS(&a));
        }
        if (use_silk) {
            SILKInfo si;
            si.signalType = (int)(lcg(&silk) % 3);
            si.offset = (int)(lcg(&silk) % 200);
            celt_encoder_ctl(st, CELT_SET_SILK_INFO(&si));
        }
        if (use_mask) {
            for (int i = 0; i < 2 * 21; i++) energy_mask[i] = (float)((int)(lcg(&mask) >> 22) - 768) * (1.f / 256.f);
            celt_encoder_ctl(st, OPUS_SET_ENERGY_MASK(energy_mask));
        }
        int r;
        memset(pkt, 0, 1276 * 2);
        if (use_pre) {
            ec_enc enc;
            ec_enc_init(&enc, pkt, bytes);
            int k = (int)(lcg(&pre) % 64);
            for (int i = 0; i < k; i++) {
                unsigned int w = lcg(&pre);
                ec_enc_bit_logp(&enc, (w >> 20) & 1, 1 + (int)((w >> 24) % 4));
            }
            /* raw bits too: past 32 of them the coder has bytes at the buffer's
               end, which every later ec_enc_shrink moves */
            int kr = (int)(lcg(&pre) % 4);
            for (int i = 0; i < kr; i++) ec_enc_bits(&enc, lcg(&pre) & 0xFFFF, 16);
            r = celt_encode_with_ec(st, pcm + pos * ch, n, NULL, bytes, &enc);
        } else {
            r = celt_encode_with_ec(st, pcm + pos * ch, n, pkt, bytes, NULL);
        }
        if (r < 0) { fprintf(stderr, "celt_encode_with_ec: %d at frame %d\n", r, count); return 1; }
        unsigned int L = (unsigned int)r, rng = 0;
        celt_encoder_ctl(st, OPUS_GET_FINAL_RANGE(&rng));
        fwrite(&L, 4, 1, fo); fwrite(pkt, 1, L, fo);
        fwrite(&rng, 4, 1, fr);
        pos += n;
        count++;
    }
    fclose(fo); fclose(fr);
    printf("%d frames\n", count);
    return 0;
}
