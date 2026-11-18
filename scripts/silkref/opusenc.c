/* opusenc: libopus's opus_encode_float with OPUS_APPLICATION_RESTRICTED_SILK,
 * the reference for shravan's Opus encoder layer (src/opus_enc.cyr).
 *   opusenc <in.f64> <out.pk> <fs> <channels> [key=value ...]
 * in.f64: interleaved doubles, passed to libopus as floats. out.pk: per packet,
 * u32le length then the packet; out.pk.rng: per packet, u32le words: the final
 * range, then OPUS_GET_BANDWIDTH, OPUS_GET_IN_DTX and OPUS_GET_BITRATE.
 * Keys (libopus's ctls; defaults are libopus's):
 *   bitrate=N|auto|max  vbr=0|1  cx=0..10  maxbw=nb|mb|wb|swb|fb  bw=auto|nb|...
 *   force=auto|1|2  signal=auto|voice|music  dtx=0|1  fec=0|1|2  loss=0..100
 *   lsb=8..24  pred=0|1 (OPUS_SET_PREDICTION_DISABLED)  maxbytes=N (the buffer; 1276*6)
 *   ms=N (the frame, ms; 20)  frames=ms,ms,... (each packet's in turn, cycled)
 *   rates=rate*count,... (each packet's bit rate in turn, cycled)
 *   chans=a12... (each packet's forced channels in turn: a auto, 1, 2)
 *   total=N (packets; default as many as the input holds)
 *   vbrc=0|1 (OPUS_SET_VBR_CONSTRAINT)  dur=5000..5009 (OPUS_SET_EXPERT_FRAME_DURATION)
 *   reset=N (OPUS_RESET_STATE before every Nth packet)
 * Also built against the system libopus by scripts/interop.sh (build/opusenc-ref):
 * a libopus without RESTRICTED_SILK (before 1.6) runs OPUS_APPLICATION_AUDIO with
 * the mode forced to SILK and the bandwidth to wideband at most, close to it (the
 * same high-pass and decisions; the tonality analysis from complexity 7). The first
 * line of output says which. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "opus.h"

#ifndef OPUS_APPLICATION_RESTRICTED_SILK
#define OPUSENC_FALLBACK 1
#define OPUS_SET_FORCE_MODE_REQUEST 11002          /* opus_private.h */
#define MODE_SILK_ONLY 1000
#endif

static int bwval(const char *s) {
    if (!strcmp(s, "nb")) return OPUS_BANDWIDTH_NARROWBAND;
    if (!strcmp(s, "mb")) return OPUS_BANDWIDTH_MEDIUMBAND;
    if (!strcmp(s, "wb")) return OPUS_BANDWIDTH_WIDEBAND;
    if (!strcmp(s, "swb")) return OPUS_BANDWIDTH_SUPERWIDEBAND;
    if (!strcmp(s, "fb")) return OPUS_BANDWIDTH_FULLBAND;
    return OPUS_AUTO;
}

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
    if (argc < 5) { fprintf(stderr, "usage: opusenc in.f64 out.pk fs channels [key=value ...]\n"); return 2; }
    int fs = atoi(argv[3]), ch = atoi(argv[4]), err;
#ifdef OPUSENC_FALLBACK
    OpusEncoder *enc = opus_encoder_create(fs, ch, OPUS_APPLICATION_AUDIO, &err);
    if (!enc) { fprintf(stderr, "create failed: %d\n", err); return 1; }
    opus_encoder_ctl(enc, OPUS_SET_FORCE_MODE_REQUEST, MODE_SILK_ONLY);
    opus_encoder_ctl(enc, OPUS_SET_MAX_BANDWIDTH(OPUS_BANDWIDTH_WIDEBAND));
    printf("mode: AUDIO forced to SILK\n");
#else
    OpusEncoder *enc = opus_encoder_create(fs, ch, OPUS_APPLICATION_RESTRICTED_SILK, &err);
    if (!enc) { fprintf(stderr, "create failed: %d\n", err); return 1; }
    printf("mode: RESTRICTED_SILK\n");
#endif
    int maxbytes = 1276 * 6, ms = 20, total = -1, reset = 0;
    static int frames[4096], rates[4096];
    int nframes = 0, nrates = 0;
    const char *chans = NULL;
    for (int i = 5; i < argc; i++) {
        char *k = argv[i], *v = strchr(k, '=');
        if (!v) { fprintf(stderr, "bad option %s\n", k); return 2; }
        *v++ = 0;
        int r = OPUS_OK;
        if (!strcmp(k, "bitrate")) r = opus_encoder_ctl(enc, OPUS_SET_BITRATE(!strcmp(v, "auto") ? OPUS_AUTO : !strcmp(v, "max") ? OPUS_BITRATE_MAX : atoi(v)));
        else if (!strcmp(k, "vbr")) r = opus_encoder_ctl(enc, OPUS_SET_VBR(atoi(v)));
        else if (!strcmp(k, "cx")) r = opus_encoder_ctl(enc, OPUS_SET_COMPLEXITY(atoi(v)));
        else if (!strcmp(k, "maxbw")) r = opus_encoder_ctl(enc, OPUS_SET_MAX_BANDWIDTH(bwval(v)));
        else if (!strcmp(k, "bw")) r = opus_encoder_ctl(enc, OPUS_SET_BANDWIDTH(bwval(v)));
        else if (!strcmp(k, "force")) r = opus_encoder_ctl(enc, OPUS_SET_FORCE_CHANNELS(!strcmp(v, "auto") ? OPUS_AUTO : atoi(v)));
        else if (!strcmp(k, "signal")) r = opus_encoder_ctl(enc, OPUS_SET_SIGNAL(!strcmp(v, "voice") ? OPUS_SIGNAL_VOICE : !strcmp(v, "music") ? OPUS_SIGNAL_MUSIC : OPUS_AUTO));
        else if (!strcmp(k, "dtx")) r = opus_encoder_ctl(enc, OPUS_SET_DTX(atoi(v)));
        else if (!strcmp(k, "fec")) r = opus_encoder_ctl(enc, OPUS_SET_INBAND_FEC(atoi(v)));
        else if (!strcmp(k, "loss")) r = opus_encoder_ctl(enc, OPUS_SET_PACKET_LOSS_PERC(atoi(v)));
        else if (!strcmp(k, "lsb")) r = opus_encoder_ctl(enc, OPUS_SET_LSB_DEPTH(atoi(v)));
        else if (!strcmp(k, "pred")) r = opus_encoder_ctl(enc, OPUS_SET_PREDICTION_DISABLED(atoi(v)));
        else if (!strcmp(k, "maxbytes")) maxbytes = atoi(v);
        else if (!strcmp(k, "ms")) ms = atoi(v);
        else if (!strcmp(k, "frames")) nframes = list(v, frames, 4096);
        else if (!strcmp(k, "rates")) nrates = list(v, rates, 4096);
        else if (!strcmp(k, "chans")) chans = v;
        else if (!strcmp(k, "total")) total = atoi(v);
        else if (!strcmp(k, "vbrc")) r = opus_encoder_ctl(enc, OPUS_SET_VBR_CONSTRAINT(atoi(v)));
        else if (!strcmp(k, "dur")) r = opus_encoder_ctl(enc, OPUS_SET_EXPERT_FRAME_DURATION(atoi(v)));
        else if (!strcmp(k, "reset")) reset = atoi(v);
        else { fprintf(stderr, "bad option %s\n", k); return 2; }
        if (r != OPUS_OK) { fprintf(stderr, "ctl %s=%s: %d\n", k, v, r); return 1; }
    }
    FILE *fi = fopen(argv[1], "rb"), *fo = fopen(argv[2], "wb");
    char rpath[4096];
    snprintf(rpath, sizeof rpath, "%s.rng", argv[2]);
    FILE *fr = fopen(rpath, "wb");
    if (!fi || !fo || !fr) { fprintf(stderr, "cannot open files\n"); return 2; }
    fseek(fi, 0, SEEK_END); long len = ftell(fi) / 8 / ch; fseek(fi, 0, SEEK_SET);
    double *d = malloc(len * ch * 8 + 8);
    if (fread(d, 8, len * ch, fi) != (size_t)(len * ch)) return 2;
    float *buf = malloc(fs / 1000 * 120 * ch * sizeof(float) + 16);
    unsigned char *pkt = malloc(maxbytes + 16);
    long pos = 0;
    int packets = 0;
    for (;;) {
        if (total >= 0 && packets >= total) break;
        int fms = nframes ? frames[packets % nframes] : ms;
        int n = fs / 1000 * fms;
        if (fms == 25) n = fs / 400;                     /* 2.5 ms as 25 */
        if (pos + n > len) break;
        if (reset > 0 && packets > 0 && packets % reset == 0) opus_encoder_ctl(enc, OPUS_RESET_STATE);
        if (nrates) opus_encoder_ctl(enc, OPUS_SET_BITRATE(rates[packets % nrates]));
        if (chans) {
            char c = chans[packets % strlen(chans)];
            opus_encoder_ctl(enc, OPUS_SET_FORCE_CHANNELS(c == 'a' ? OPUS_AUTO : c - '0'));
        }
        for (int i = 0; i < n * ch; i++) buf[i] = (float)d[pos * ch + i];
        int r = opus_encode_float(enc, buf, n, pkt, maxbytes);
        if (r < 0) { fprintf(stderr, "opus_encode_float: %d at packet %d\n", r, packets); return 1; }
        unsigned int L = (unsigned int)r, rng = 0;
        opus_int32 aux[3];
        opus_encoder_ctl(enc, OPUS_GET_FINAL_RANGE(&rng));
        opus_encoder_ctl(enc, OPUS_GET_BANDWIDTH(&aux[0]));
        opus_encoder_ctl(enc, OPUS_GET_IN_DTX(&aux[1]));
        opus_encoder_ctl(enc, OPUS_GET_BITRATE(&aux[2]));
        fwrite(&L, 4, 1, fo); fwrite(pkt, 1, L, fo);
        fwrite(&rng, 4, 1, fr); fwrite(aux, 4, 3, fr);
        pos += n;
        packets++;
    }
    fclose(fo); fclose(fr);
    printf("%d packets\n", packets);
    return 0;
}
