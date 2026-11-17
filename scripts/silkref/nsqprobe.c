/* nsqprobe: libopus's silk_NSQ_c on synthetic input that the encoder never
 * produces on its own: large residuals (small gains) at Lambda_Q10 above 2048,
 * the quantiser's aggressive rate/distortion branch beyond its sign decision.
 * Unvoiced and voiced (LTP, re-whitening), wideband 20 ms. Run with
 * SILKDUMP=<file>: patch.py's hooks in silk_NSQ_c record each call. The gains
 * put the residuals at a few pulses: much larger ones saturate at the limit of
 * 31 pulses, where any bias picks the same level. */
#include <stdio.h>
#include <string.h>
#include "main.h"

static unsigned int seed = 12345;
static int rnd(int range) { seed = seed * 1103515245u + 12345u; return (int)((seed >> 8) % (unsigned)(2 * range + 1)) - range; }

int main(void) {
    static silk_encoder_state enc;
    static silk_nsq_state nsq;
    SideInfoIndices ind;
    opus_int16 x16[320], pred[2 * MAX_LPC_ORDER], ltp[LTP_ORDER * MAX_NB_SUBFR], ar[MAX_NB_SUBFR * MAX_SHAPE_LPC_ORDER];
    opus_int harm[4], tilt[4], pitchL[4];
    opus_int32 lf[4], gains[4];
    opus_int8 pulses[320];
    int call, i, k;
    for (call = 0; call < 4; call++) {
        memset(&enc, 0, sizeof enc);
        memset(&ind, 0, sizeof ind);
        if (call == 0) { memset(&nsq, 0, sizeof nsq); nsq.lagPrev = 100; nsq.prev_gain_Q16 = 65536; }
        enc.ltp_mem_length = 320; enc.frame_length = 320; enc.subfr_length = 80; enc.nb_subfr = 4;
        enc.shapingLPCOrder = 14; enc.predictLPCOrder = 16; enc.frameCounter = call;
        ind.Seed = call & 3; ind.signalType = call >= 2 ? TYPE_VOICED : TYPE_UNVOICED;
        ind.quantOffsetType = call & 1; ind.NLSFInterpCoef_Q2 = 4;
        for (i = 0; i < 320; i++) x16[i] = (opus_int16)rnd(20000);
        for (i = 0; i < 2 * MAX_LPC_ORDER; i++) pred[i] = (opus_int16)(i % MAX_LPC_ORDER < 16 ? rnd(600) : 0);
        for (i = 0; i < LTP_ORDER * MAX_NB_SUBFR; i++) ltp[i] = (opus_int16)(call >= 2 ? (i % 5 == 2 ? 9000 : rnd(1500)) : 0);
        for (i = 0; i < MAX_NB_SUBFR * MAX_SHAPE_LPC_ORDER; i++) ar[i] = (opus_int16)rnd(2000);
        for (k = 0; k < 4; k++) {
            harm[k] = call >= 2 ? 4000 : 0; tilt[k] = -4096;
            lf[k] = (opus_int32)((opus_uint32)(14000) << 16 | (opus_uint16)(-15000));
            gains[k] = (2500 + 700 * k) << 16;  /* residuals of a few pulses: where the bias decides */
            pitchL[k] = call >= 2 ? 90 + 3 * k : 0;
        }
        /* Lambda_Q10 2300 (just past the branch's threshold: without the bias the
           two candidate levels miss the cost's minimum), 6000 and 3000 */
        silk_NSQ_c(&enc, &nsq, &ind, x16, pulses, pred, ltp, ar, harm, tilt, lf, gains, pitchL,
                   call == 1 ? 6000 : (call == 3 ? 3000 : 2300), call >= 2 ? 12288 : 0);
    }
    printf("4 calls\n");
    return 0;
}
