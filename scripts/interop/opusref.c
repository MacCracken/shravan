/* scripts/interop/opusref.c -- libopus reference vectors for the interop sweep.
 *
 * Judging shravan against libopus's own decode of the same packets removes every
 * container, pre-skip and resampling variable. Build (needs libopus headers):
 *
 *     cc -O2 -o build/opusref scripts/interop/opusref.c -lopus -lm
 *
 * or against a libopus installed under a prefix (scripts/interop/build-libopus.sh
 * makes a default build): -I$OPUS_PREFIX/include $OPUS_PREFIX/lib/libopus.a.
 *
 *   opusref gen <config 0-31> <channels> <packets> <out.pk> <out.f64> [key=value ...]
 *       Encode a broadband test signal with libopus, forcing the mode, bandwidth
 *       and frame duration of TOC <config>. Writes the packets (each prefixed by a
 *       u32le length) and libopus's opus_decode_float output for exactly those
 *       packets as f64. Prints a histogram of the TOC configs actually produced,
 *       and of mono/stereo packets: libopus can switch mid-stream, and a mixed
 *       stream must not be mistaken for the one asked for. Options:
 *         dur=N   packet duration in 48 kHz samples; longer than the config's
 *                 frame gives multi-frame packets (codes 1-3)
 *         fec=P   inband FEC on for P% expected loss (SILK LBRR data)
 *         cbr=1   constant bit rate
 *         br=B    bit rate in bits/s (default 32k SILK, 64k otherwise, per channel)
 *         dtx=1   discontinuous transmission (1-byte packets in silence)
 *         sig=1   channels alternate between identical and independent (stereo
 *                 SILK mid-only frames and the side channel restarting)
 *         sig=2   bursts separated by digital silence (voice-inactive frames)
 *         sig=3   0.35 s bursts with 0.65 s of digital silence (long enough for
 *                 DTX, which libopus starts after 200 ms of inactivity)
 *         sw=N    cycle the forced mode every N packets through SILK WB, hybrid
 *                 FB, SILK WB, CELT FB, hybrid FB, CELT FB: all six transitions
 *         swmb=N  alternate every N packets between mediumband SILK and fullband
 *                 CELT only (mode switches with no 16 kHz SILK in the stream)
 *         chsw=N  alternate mono and stereo coding every N packets
 *         bwsw=N  cycle the bandwidth every N packets through those the forced
 *                 mode allows (SILK NB/MB/WB, hybrid FB/SWB, CELT FB/WB/SWB/NB)
 *         fsw=N   alternate the frame duration between the config's and twice
 *                 it every N packets
 *         auto=1  no forcing at all: libopus picks mode, bandwidth and channels
 *         voip=1  OPUS_APPLICATION_VOIP instead of AUDIO
 *         lose=N  every Nth packet is lost: written as an empty packet, and the
 *                 reference conceals it (opus_decode with no data, for the
 *                 previous packet's duration)
 *   opusref caps
 *       Print the library version, its decoder size and whether it is a deep-PLC
 *       build (deep_plc 1), whose wideband SILK concealment differs by design.
 *
 *   opusref dec <in.pk> <channels> <out.f64>
 *       Decode packets with libopus (used to judge shravan's encoder).
 *
 * opus_decode_float, not opus_decode: the int16 path runs opus_pcm_soft_clip,
 * a limiter that is not part of the codec and pulls loud output down by up to 20%.
 */
#include <math.h>
#include <opus/opus.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define FORCE_MODE_REQUEST 11002 /* OPUS_SET_FORCE_MODE, opus_private.h */
#define MODE_SILK_ONLY 1000
#define MODE_HYBRID 1001
#define MODE_CELT_ONLY 1002
#define MAX_PACKET 1500
#define MAX_FRAME 5760 /* 120 ms at 48 kHz */

static void put_u32(FILE *f, uint32_t v) {
    unsigned char b[4] = {v & 255, (v >> 8) & 255, (v >> 16) & 255, v >> 24};
    fwrite(b, 1, 4, f);
}

static int get_u32(FILE *f, uint32_t *v) {
    unsigned char b[4];
    if (fread(b, 1, 4, f) != 4) return 0;
    *v = b[0] | (b[1] << 8) | (b[2] << 16) | ((uint32_t)b[3] << 24);
    return 1;
}

/* Mode, bandwidth and frame duration (in 48 kHz samples) of a TOC config. */
static void config_params(int config, int *mode, int *bw, int *frame) {
    static const int silk_bw[3] = {OPUS_BANDWIDTH_NARROWBAND, OPUS_BANDWIDTH_MEDIUMBAND,
                                   OPUS_BANDWIDTH_WIDEBAND};
    static const int celt_bw[4] = {OPUS_BANDWIDTH_NARROWBAND, OPUS_BANDWIDTH_WIDEBAND,
                                   OPUS_BANDWIDTH_SUPERWIDEBAND, OPUS_BANDWIDTH_FULLBAND};
    static const int silk_frame[4] = {480, 960, 1920, 2880};
    static const int celt_frame[4] = {120, 240, 480, 960};
    if (config < 12) {
        *mode = MODE_SILK_ONLY;
        *bw = silk_bw[config / 4];
        *frame = silk_frame[config % 4];
    } else if (config < 16) {
        *mode = MODE_HYBRID;
        *bw = config < 14 ? OPUS_BANDWIDTH_SUPERWIDEBAND : OPUS_BANDWIDTH_FULLBAND;
        *frame = config % 2 ? 960 : 480;
    } else {
        *mode = MODE_CELT_ONLY;
        *bw = celt_bw[(config - 16) / 4];
        *frame = celt_frame[config % 4];
    }
}

/* Tones across the band, a sweep, noise and bursts, different per channel. */
static void make_signal(float *pcm, int n, int channels, int sig) {
    uint32_t seed = 12345;
    for (int i = 0; i < n; i++) {
        double t = i / 48000.0;
        for (int c = 0; c < channels; c++) {
            seed = seed * 1664525u + 1013904223u;
            double noise = ((seed >> 8) / 16777216.0 - 0.5) * 0.08;
            double burst = fmod(t + 0.07 * c, 0.23) < 0.02 ? 0.25 : 0.0;
            double v = 0.18 * sin(2 * M_PI * (220.0 + 110 * c) * t) +
                       0.10 * sin(2 * M_PI * 1830.0 * t + c) +
                       0.07 * sin(2 * M_PI * 6100.0 * t) +
                       0.05 * sin(2 * M_PI * (300.0 + 9000.0 * fmod(t, 1.0)) * t) +
                       noise * (1.0 + 4.0 * burst) + burst * sin(2 * M_PI * 3500.0 * t);
            if (sig == 1 && c == 1 && fmod(t, 0.4) < 0.2) v = pcm[i * channels];
            if (sig == 2 && fmod(t, 0.3) >= 0.12) v = 0.0;
            if (sig == 3 && fmod(t, 1.0) >= 0.35) v = 0.0;
            pcm[i * channels + c] = (float)v;
        }
    }
}

struct opts { int dur, fec, cbr, br, dtx, sig, sw, chsw, bwsw, fsw, autom, voip, lose, swmb; };

static int gen(int config, int channels, int frames, const char *pk_path, const char *ref_path,
               const struct opts *o) {
    int mode, bw, frame, err;
    config_params(config, &mode, &bw, &frame);
    OpusEncoder *enc = opus_encoder_create(48000, channels,
        o->voip ? OPUS_APPLICATION_VOIP : OPUS_APPLICATION_AUDIO, &err);
    OpusDecoder *dec = opus_decoder_create(48000, channels, &err);
    if (!enc || !dec) return fprintf(stderr, "opus create failed\n"), 2;
    if (!o->autom) {
        opus_encoder_ctl(enc, FORCE_MODE_REQUEST, mode);
        opus_encoder_ctl(enc, OPUS_SET_BANDWIDTH(bw));
        opus_encoder_ctl(enc, OPUS_SET_FORCE_CHANNELS(channels));
    }
    int br = o->br ? o->br : channels * (mode == MODE_SILK_ONLY ? 32000 : 64000);
    opus_encoder_ctl(enc, OPUS_SET_BITRATE(br));
    opus_encoder_ctl(enc, OPUS_SET_COMPLEXITY(10));
    if (o->fec > 0) {
        opus_encoder_ctl(enc, OPUS_SET_INBAND_FEC(1));
        opus_encoder_ctl(enc, OPUS_SET_PACKET_LOSS_PERC(o->fec));
    }
    if (o->cbr) opus_encoder_ctl(enc, OPUS_SET_VBR(0));
    if (o->dtx) opus_encoder_ctl(enc, OPUS_SET_DTX(1));
    if (o->dur > 0) frame = o->dur;

    int maxframe = o->fsw ? 2 * frame : frame;
    int n = maxframe * frames;
    float *pcm = malloc(sizeof(float) * n * channels);
    float *out = malloc(sizeof(float) * MAX_FRAME * channels);
    make_signal(pcm, n, channels, o->sig);
    FILE *fp = fopen(pk_path, "wb"), *fr = fopen(ref_path, "wb");
    if (!fp || !fr) return fprintf(stderr, "cannot write output\n"), 2;
    int hist[32] = {0}, stereo_pk = 0, short_pk = 0, lost_pk = 0, last_dur = 960;
    unsigned char pkt[MAX_PACKET];
    int pos = 0;
    for (int k = 0; k < frames; k++) {
        if (o->sw > 0) {
            static const int sw_mode[6] = {MODE_SILK_ONLY, MODE_HYBRID, MODE_SILK_ONLY,
                                           MODE_CELT_ONLY, MODE_HYBRID, MODE_CELT_ONLY};
            static const int sw_bw[6] = {OPUS_BANDWIDTH_WIDEBAND, OPUS_BANDWIDTH_FULLBAND,
                                         OPUS_BANDWIDTH_WIDEBAND, OPUS_BANDWIDTH_FULLBAND,
                                         OPUS_BANDWIDTH_FULLBAND, OPUS_BANDWIDTH_FULLBAND};
            int ph = (k / o->sw) % 6;
            opus_encoder_ctl(enc, FORCE_MODE_REQUEST, sw_mode[ph]);
            opus_encoder_ctl(enc, OPUS_SET_BANDWIDTH(sw_bw[ph]));
        }
        if (o->swmb > 0) {
            int ph = (k / o->swmb) % 2;
            opus_encoder_ctl(enc, FORCE_MODE_REQUEST, ph ? MODE_CELT_ONLY : MODE_SILK_ONLY);
            opus_encoder_ctl(enc, OPUS_SET_BANDWIDTH(ph ? OPUS_BANDWIDTH_FULLBAND : OPUS_BANDWIDTH_MEDIUMBAND));
        }
        if (o->chsw > 0)
            opus_encoder_ctl(enc, OPUS_SET_FORCE_CHANNELS((k / o->chsw) % 2 ? 1 : channels));
        if (o->bwsw > 0) {
            /* The bandwidths the forced mode allows. */
            static const int silk_bws[3] = {OPUS_BANDWIDTH_NARROWBAND, OPUS_BANDWIDTH_MEDIUMBAND,
                                            OPUS_BANDWIDTH_WIDEBAND};
            static const int celt_bws[4] = {OPUS_BANDWIDTH_FULLBAND, OPUS_BANDWIDTH_WIDEBAND,
                                            OPUS_BANDWIDTH_SUPERWIDEBAND, OPUS_BANDWIDTH_NARROWBAND};
            static const int hyb_bws[2] = {OPUS_BANDWIDTH_FULLBAND, OPUS_BANDWIDTH_SUPERWIDEBAND};
            int ph = k / o->bwsw, b;
            if (mode == MODE_SILK_ONLY) b = silk_bws[ph % 3];
            else if (mode == MODE_HYBRID) b = hyb_bws[ph % 2];
            else b = celt_bws[ph % 4];
            opus_encoder_ctl(enc, OPUS_SET_BANDWIDTH(b));
        }
        int fs = frame;
        if (o->fsw > 0 && (k / o->fsw) % 2) fs = 2 * frame;
        int len = opus_encode_float(enc, pcm + pos * channels, fs, pkt, MAX_PACKET);
        pos += fs;
        if (len < 1) return fprintf(stderr, "encode failed: %d\n", len), 2;
        int got;
        if (o->lose > 0 && k > 0 && k % o->lose == o->lose - 1) {
            put_u32(fp, 0);
            got = opus_decode_float(dec, NULL, 0, out, last_dur, 0);
            lost_pk++;
        } else {
            hist[pkt[0] >> 3]++;
            stereo_pk += (pkt[0] >> 2) & 1;
            short_pk += len <= 2;
            put_u32(fp, len);
            fwrite(pkt, 1, len, fp);
            got = opus_decode_float(dec, pkt, len, out, MAX_FRAME, 0);
            last_dur = got;
        }
        if (got < 0) return fprintf(stderr, "decode failed: %d\n", got), 2;
        for (int i = 0; i < got * channels; i++) {
            double d = out[i];
            fwrite(&d, 8, 1, fr);
        }
    }
    fclose(fp);
    fclose(fr);
    printf("configs:");
    for (int c = 0; c < 32; c++)
        if (hist[c]) printf(" %d x%d", c, hist[c]);
    printf(" | stereo %d/%d", stereo_pk, frames);
    if (short_pk) printf(" | dtx %d", short_pk);
    if (lost_pk) printf(" | lost %d", lost_pk);
    printf("\n");
    return 0;
}

static int parse_opt(struct opts *o, const char *a) {
    static const char *names[] = {"dur", "fec", "cbr", "br", "dtx", "sig", "sw",
                                  "chsw", "bwsw", "fsw", "auto", "voip", "lose", "swmb"};
    int *fields[] = {&o->dur, &o->fec, &o->cbr, &o->br, &o->dtx, &o->sig, &o->sw,
                     &o->chsw, &o->bwsw, &o->fsw, &o->autom, &o->voip, &o->lose, &o->swmb};
    const char *eq = strchr(a, '=');
    if (!eq) return 0;
    for (unsigned i = 0; i < sizeof(names) / sizeof(names[0]); i++) {
        if (strlen(names[i]) == (size_t)(eq - a) && !strncmp(a, names[i], eq - a)) {
            *fields[i] = atoi(eq + 1);
            return 1;
        }
    }
    return 0;
}

static int dec_file(const char *pk_path, int channels, const char *out_path) {
    int err;
    OpusDecoder *dec = opus_decoder_create(48000, channels, &err);
    FILE *fp = fopen(pk_path, "rb"), *fo = fopen(out_path, "wb");
    if (!dec || !fp || !fo) return fprintf(stderr, "open failed\n"), 2;
    float *out = malloc(sizeof(float) * MAX_FRAME * channels);
    unsigned char pkt[MAX_PACKET];
    uint32_t len;
    int packets = 0;
    while (get_u32(fp, &len)) {
        if (len > MAX_PACKET || fread(pkt, 1, len, fp) != len) return fprintf(stderr, "bad packet\n"), 2;
        int got = opus_decode_float(dec, pkt, len, out, MAX_FRAME, 0);
        if (got < 0) return fprintf(stderr, "packet %d: decode failed: %d\n", packets, got), 3;
        for (int i = 0; i < got * channels; i++) {
            double d = out[i];
            fwrite(&d, 8, 1, fo);
        }
        packets++;
    }
    fclose(fo);
    printf("packets: %d\n", packets);
    return 0;
}

int main(int argc, char **argv) {
    if (argc >= 7 && !strcmp(argv[1], "gen")) {
        struct opts o;
        memset(&o, 0, sizeof o);
        for (int i = 7; i < argc; i++)
            if (!parse_opt(&o, argv[i])) return fprintf(stderr, "bad option: %s\n", argv[i]), 2;
        return gen(atoi(argv[2]), atoi(argv[3]), atoi(argv[4]), argv[5], argv[6], &o);
    }
    if (argc == 5 && !strcmp(argv[1], "dec"))
        return dec_file(argv[2], atoi(argv[3]), argv[4]);
    if (argc == 2 && !strcmp(argv[1], "caps")) {
        /* A deep-PLC build (DRED or OSCE) carries LPCNet state in every decoder:
           about 155 KB a mono decoder against 18 KB for a default build. Its SILK
           PLC glue also skips the fade-in at 16 kHz, so wideband concealment can
           only be compared with a default build. The DRED entry points cannot
           tell them apart: 1.6.1 exports them as stubs either way. */
        int size = opus_decoder_get_size(1);
        printf("%s | decoder %d bytes | deep_plc %d\n", opus_get_version_string(), size,
               size > 65536);
        return 0;
    }
    fprintf(stderr, "usage: opusref gen <config> <channels> <packets> <out.pk> <out.f64> [key=value ...]\n"
                    "       opusref dec <in.pk> <channels> <out.f64>\n"
                    "       opusref caps\n");
    return 2;
}
