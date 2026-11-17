/* silkref: libopus's SILK encoder on a 16-bit stream.
 *   silkref <in.s16> <out.pk> <rate_hz> <bitrate> <complexity> <frame_ms> [cbr [maxbytes [loss [fec [dtx [api_hz [channels [sched [rates]]]]]]]]]
 * out.pk: per packet, u32le length then the SILK payload (no TOC; length 0 for
 * a packet DTX leaves out). rate_hz is the internal rate, api_hz the input's
 * (default rate_hz). maxbytes caps a VBR packet (maxBits; default 1275 bytes),
 * as shravan's silk_encode takes it from its buffer; loss is
 * packetLossPercentage (default 0); fec sets useInBandFEC and LBRR_coded, dtx
 * useDTX. channels: the input's, 1 or 2 (interleaved; nChannelsAPI). sched:
 * the packets' coded channels in turn, cycled: 1 mono, 2 stereo, 3 stereo with
 * toMono (default: channels). rates: the packets' bit rates in turn, cycled,
 * as rate*count,... (default: bitrate throughout). With SILKDUMP=<file> the
 * instrumented library records each stage. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "opus_types.h"
#include "API.h"
#include "entenc.h"

int main(int argc, char **argv) {
    if (argc < 7) { fprintf(stderr, "usage: silkref in.s16 out.pk rate bitrate complexity frame_ms [cbr]\n"); return 2; }
    FILE *fi = fopen(argv[1], "rb"), *fo = fopen(argv[2], "wb");
    if (!fi || !fo) { fprintf(stderr, "cannot open files\n"); return 2; }
    int rate = atoi(argv[3]), bitrate = atoi(argv[4]), complexity = atoi(argv[5]), ms = atoi(argv[6]);
    int cbr = argc > 7 ? atoi(argv[7]) : 0;
    int maxbytes = argc > 8 ? atoi(argv[8]) : 1275;
    int loss = argc > 9 ? atoi(argv[9]) : 0;
    int fec = argc > 10 ? atoi(argv[10]) : 0;
    int dtx = argc > 11 ? atoi(argv[11]) : 0;
    int api = argc > 12 ? atoi(argv[12]) : rate;
    int ch = argc > 13 ? atoi(argv[13]) : 1;
    const char *sched = argc > 14 ? argv[14] : (ch == 2 ? "2" : "1");
    int nsched = (int)strlen(sched);
    static int rates[4096]; int nrates = 0;
    if (argc > 15) {
        const char *q = argv[15];
        while (*q && nrates < 4096) {
            char *e; long r = strtol(q, &e, 10), c = 1;
            if (*e == '*') c = strtol(e + 1, &e, 10);
            while (c-- > 0 && nrates < 4096) rates[nrates++] = (int)r;
            q = *e == ',' ? e + 1 : e;
            if (e == q && *q) break;
        }
    }
    if ((ch != 1 && ch != 2) || nsched < 1) { fprintf(stderr, "bad channels\n"); return 2; }
    fseek(fi, 0, SEEK_END); long len = ftell(fi) / 2 / ch; fseek(fi, 0, SEEK_SET);
    short *pcm = malloc(len * 2 * ch + 2);
    if (fread(pcm, 2, len * ch, fi) != (size_t)(len * ch)) return 2;
    int size; silk_Get_Encoder_Size(&size, ch);
    void *enc = calloc(1, size);
    silk_EncControlStruct ctl; memset(&ctl, 0, sizeof ctl);
    if (silk_InitEncoder(enc, ch, 0, &ctl)) { fprintf(stderr, "init failed\n"); return 1; }
    ctl.nChannelsAPI = ch; ctl.nChannelsInternal = ch;
    ctl.API_sampleRate = api; ctl.maxInternalSampleRate = rate; ctl.minInternalSampleRate = rate;
    ctl.desiredInternalSampleRate = rate; ctl.payloadSize_ms = ms; ctl.bitRate = bitrate;
    ctl.packetLossPercentage = loss; ctl.complexity = complexity; ctl.useInBandFEC = fec; ctl.useDRED = 0;
    ctl.LBRR_coded = fec; ctl.useDTX = dtx; ctl.useCBR = cbr; ctl.toMono = 0; ctl.opusCanSwitch = 0;
    ctl.reducedDependency = 0;
    ctl.maxBits = cbr ? bitrate * ms / 1000 : maxbytes * 8;
    int n = api * ms / 1000;
    float *buf = malloc(n * ch * sizeof(float));
    unsigned char data[1275];
    long p; int packets = 0;
    for (p = 0; p + n <= len; p += n) {
        int i; for (i = 0; i < n * ch; i++) buf[i] = pcm[p * ch + i] * (1.0f / 32768.0f);
        char c = sched[packets % nsched];
        ctl.nChannelsInternal = c == '1' ? 1 : 2;
        ctl.toMono = c == '3';
        if (nrates) {
            ctl.bitRate = rates[packets % nrates];
            ctl.maxBits = cbr ? ctl.bitRate * ms / 1000 : maxbytes * 8;
        }
        ec_enc e; ec_enc_init(&e, data, 1275);
        opus_int32 nBytes = 1275;
        int r = silk_Encode(enc, &ctl, buf, n, &e, &nBytes, 0, -1);
        if (r) { fprintf(stderr, "silk_Encode error %d\n", r); return 1; }
        ec_enc_done(&e);
        unsigned int L = (unsigned int)nBytes;
        fwrite(&L, 4, 1, fo); fwrite(data, 1, L, fo);
        packets++;
    }
    fclose(fo);
    printf("%d packets\n", packets);
    return 0;
}
