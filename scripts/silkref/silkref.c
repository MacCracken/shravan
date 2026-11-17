/* silkref: libopus's SILK encoder on a mono 16-bit stream at the internal rate.
 *   silkref <in.s16> <out.pk> <rate_hz> <bitrate> <complexity> <frame_ms> [cbr [maxbytes]]
 * out.pk: per packet, u32le length then the SILK payload (no TOC). maxbytes
 * caps a VBR packet (maxBits; default 1275 bytes), as shravan's silk_encode
 * takes it from its buffer. With SILKDUMP=<file> the instrumented library
 * records each stage. */
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
    fseek(fi, 0, SEEK_END); long len = ftell(fi) / 2; fseek(fi, 0, SEEK_SET);
    short *pcm = malloc(len * 2 + 2);
    if (fread(pcm, 2, len, fi) != (size_t)len) return 2;
    int size; silk_Get_Encoder_Size(&size, 1);
    void *enc = calloc(1, size);
    silk_EncControlStruct ctl; memset(&ctl, 0, sizeof ctl);
    if (silk_InitEncoder(enc, 1, 0, &ctl)) { fprintf(stderr, "init failed\n"); return 1; }
    ctl.nChannelsAPI = 1; ctl.nChannelsInternal = 1;
    ctl.API_sampleRate = rate; ctl.maxInternalSampleRate = rate; ctl.minInternalSampleRate = rate;
    ctl.desiredInternalSampleRate = rate; ctl.payloadSize_ms = ms; ctl.bitRate = bitrate;
    ctl.packetLossPercentage = 0; ctl.complexity = complexity; ctl.useInBandFEC = 0; ctl.useDRED = 0;
    ctl.LBRR_coded = 0; ctl.useDTX = 0; ctl.useCBR = cbr; ctl.toMono = 0; ctl.opusCanSwitch = 0;
    ctl.reducedDependency = 0;
    ctl.maxBits = cbr ? bitrate * ms / 1000 : maxbytes * 8;
    int n = rate * ms / 1000;
    float *buf = malloc(n * sizeof(float));
    unsigned char data[1275];
    long p; int packets = 0;
    for (p = 0; p + n <= len; p += n) {
        int i; for (i = 0; i < n; i++) buf[i] = pcm[p + i] * (1.0f / 32768.0f);
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
