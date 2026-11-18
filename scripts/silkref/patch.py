#!/usr/bin/env python3
"""Instrument a libopus source tree for scripts/gen-silk-vectors.py.

    scripts/silkref/patch.py <opus-source-dir>

Inserts silkdump.h's record writers at the SILK encoder's stage boundaries, so
that a run with SILKDUMP=<file> writes each stage's inputs and outputs, one text
record a line. Each edit is anchored on libopus v1.6.1's source text and fails
loudly if the anchor is missing.
"""
import os
import shutil
import sys

ROOT = sys.argv[1]
HERE = os.path.dirname(os.path.abspath(__file__))
shutil.copy(os.path.join(HERE, "silkdump.h"), os.path.join(ROOT, "silk", "silkdump.h"))


def patch(rel, pairs):
    path = os.path.join(ROOT, rel)
    s = open(path).read()
    if '#include "silkdump.h"' not in s:
        for inc in ('#include "main_FLP.h"', '#include "SigProc_FLP.h"', '#include "main.h"'):
            if inc in s:
                i = s.index(inc)
                s = s[:i] + '#include "silkdump.h"\n' + s[i:]
                break
        else:
            sys.exit("patch: no include anchor in " + rel)
    for old, new in pairs:
        if s.count(old) != 1:
            sys.exit("patch: anchor not found once in %s: %r" % (rel, old[:60]))
        s = s.replace(old, new)
    open(path, "w").write(s)


# 2.11.0: the unvoiced prediction path (find_pred_coefs, find_LPC, Burg).
patch("silk/float/find_pred_coefs_FLP.c", [
    ("""    /* Weighting for weighted least squares */
    for( i = 0; i < psEnc->sCmn.nb_subfr; i++ ) {""", """    {
        int ord = psEnc->sCmn.predictLPCOrder, nb = psEnc->sCmn.nb_subfr, sl = psEnc->sCmn.subfr_length;
        sd_int("fpc.frame", psEnc->sCmn.frameCounter);
        sd_int("fpc.signalType", psEnc->sCmn.indices.signalType);
        sd_int("fpc.nb_subfr", nb); sd_int("fpc.subfr_length", sl); sd_int("fpc.order", ord);
        sd_int("fpc.first", psEnc->sCmn.first_frame_after_reset);
        sd_int("fpc.useinterp", psEnc->sCmn.useInterpolatedNLSFs);
        sd_int("fpc.survivors", psEnc->sCmn.NLSF_MSVQ_Survivors);
        sd_int("fpc.fs_kHz", psEnc->sCmn.fs_kHz);
        sd_int("fpc.speech_q8", psEnc->sCmn.speech_activity_Q8);
        sd_flt("fpc.coding_quality", psEncCtrl->coding_quality);
        sd_f("fpc.gains", psEncCtrl->Gains, nb);
        sd_i16("fpc.prevnlsf", psEnc->sCmn.prev_NLSFq_Q15, ord);
        sd_f("fpc.x", x - ord, nb * sl + ord);
    }
    /* Weighting for weighted least squares */
    for( i = 0; i < psEnc->sCmn.nb_subfr; i++ ) {"""),
    ("""    /* LPC_in_pre contains the LTP-filtered input for voiced, and the unfiltered input for unvoiced */
    silk_find_LPC_FLP( &psEnc->sCmn, NLSF_Q15, LPC_in_pre, minInvGain, psEnc->sCmn.arch );""", """    sd_flt("fpc.mininvgain", minInvGain);
    sd_f("fpc.lpcinpre", LPC_in_pre, psEnc->sCmn.nb_subfr * (psEnc->sCmn.subfr_length + psEnc->sCmn.predictLPCOrder));
    /* LPC_in_pre contains the LTP-filtered input for voiced, and the unfiltered input for unvoiced */
    silk_find_LPC_FLP( &psEnc->sCmn, NLSF_Q15, LPC_in_pre, minInvGain, psEnc->sCmn.arch );
    sd_i16("fpc.nlsf_unq", NLSF_Q15, psEnc->sCmn.predictLPCOrder);
    sd_int("fpc.interp", psEnc->sCmn.indices.NLSFInterpCoef_Q2);"""),
    ("""    silk_process_NLSFs_FLP( &psEnc->sCmn, psEncCtrl->PredCoef, NLSF_Q15, psEnc->sCmn.prev_NLSFq_Q15 );""",
     """    silk_process_NLSFs_FLP( &psEnc->sCmn, psEncCtrl->PredCoef, NLSF_Q15, psEnc->sCmn.prev_NLSFq_Q15 );
    sd_i16("fpc.nlsf_q", NLSF_Q15, psEnc->sCmn.predictLPCOrder);
    sd_i8("fpc.nlsfidx", psEnc->sCmn.indices.NLSFIndices, psEnc->sCmn.predictLPCOrder + 1);
    sd_f("fpc.pred0", psEncCtrl->PredCoef[0], psEnc->sCmn.predictLPCOrder);
    sd_f("fpc.pred1", psEncCtrl->PredCoef[1], psEnc->sCmn.predictLPCOrder);"""),
    ("""    /* Copy to prediction struct for use in next frame for interpolation */""",
     """    sd_f("fpc.resnrg", psEncCtrl->ResNrg, psEnc->sCmn.nb_subfr);
    /* Copy to prediction struct for use in next frame for interpolation */"""),
])
patch("silk/float/burg_modified_FLP.c", [
    ("""    celt_assert( subfr_length * nb_subfr <= MAX_FRAME_SIZE );""",
     """    celt_assert( subfr_length * nb_subfr <= MAX_FRAME_SIZE );
    { int hdr[3]; hdr[0] = D; hdr[1] = nb_subfr; hdr[2] = subfr_length; sd_i32("burg.hdr", hdr, 3); sd_flt("burg.mininv", minInvGain); }"""),
    ("""    /* Return residual energy */
    return (silk_float)nrg_f;""", """    sd_f("burg.A", A, D);
    sd_flt("burg.nrg", (silk_float)nrg_f);
    /* Return residual energy */
    return (silk_float)nrg_f;"""),
])
# 2.11.1: the VAD, the pitch analysis front end, noise shaping and gains.
patch("silk/float/encode_frame_FLP.c", [
    ("""    silk_VAD_GetSA_Q8( &psEnc->sCmn, psEnc->sCmn.inputBuf + 1, psEnc->sCmn.arch );""",
     """    sd_int("vad.frame", psEnc->sCmn.frameCounter);
    sd_int("vad.fs", psEnc->sCmn.fs_kHz);
    sd_int("vad.fl", psEnc->sCmn.frame_length);
    sd_i16("vad.in", psEnc->sCmn.inputBuf + 1, psEnc->sCmn.frame_length);
    silk_VAD_GetSA_Q8( &psEnc->sCmn, psEnc->sCmn.inputBuf + 1, psEnc->sCmn.arch );"""),
    ("""        psEnc->sCmn.VAD_flags[ psEnc->sCmn.nFramesEncoded ] = 1;
    }""", """        psEnc->sCmn.VAD_flags[ psEnc->sCmn.nFramesEncoded ] = 1;
    }
    sd_int("vad.speech", psEnc->sCmn.speech_activity_Q8);
    sd_int("vad.tilt", psEnc->sCmn.input_tilt_Q15);
    sd_i32("vad.quality", psEnc->sCmn.input_quality_bands_Q15, 4);
    sd_int("vad.signalType", psEnc->sCmn.indices.signalType);"""),
])
patch("silk/float/find_pitch_lags_FLP.c", [
    ("""    x_buf = x - psEnc->sCmn.ltp_mem_length;""", """    x_buf = x - psEnc->sCmn.ltp_mem_length;
    {
        int h[9];
        h[0] = psEnc->sCmn.la_pitch; h[1] = psEnc->sCmn.frame_length; h[2] = psEnc->sCmn.ltp_mem_length;
        h[3] = psEnc->sCmn.pitch_LPC_win_length; h[4] = psEnc->sCmn.pitchEstimationLPCOrder;
        h[5] = psEnc->sCmn.indices.signalType; h[6] = psEnc->sCmn.first_frame_after_reset;
        h[7] = psEnc->sCmn.fs_kHz; h[8] = psEnc->sCmn.nb_subfr;
        sd_i32("fpl.hdr", h, 9);
        sd_f("fpl.x", x_buf, buf_len);
    }"""),
    ("""    silk_LPC_analysis_filter_FLP( res, A, x_buf, buf_len, psEnc->sCmn.pitchEstimationLPCOrder );""",
     """    silk_LPC_analysis_filter_FLP( res, A, x_buf, buf_len, psEnc->sCmn.pitchEstimationLPCOrder );
    sd_flt("fpl.predgain", psEncCtrl->predGain);
    sd_f("fpl.res", res, buf_len);"""),
])
patch("silk/float/noise_shape_analysis_FLP.c", [
    ("""    /* Point to start of first LPC analysis block */
    x_ptr = x - psEnc->sCmn.la_shape;""", """    {
        int h[11];
        h[0] = psEnc->sCmn.SNR_dB_Q7; h[1] = psEnc->sCmn.speech_activity_Q8; h[2] = psEnc->sCmn.useCBR;
        h[3] = psEnc->sCmn.indices.signalType; h[4] = psEnc->sCmn.fs_kHz; h[5] = psEnc->sCmn.nb_subfr;
        h[6] = psEnc->sCmn.subfr_length; h[7] = psEnc->sCmn.shapeWinLength; h[8] = psEnc->sCmn.shapingLPCOrder;
        h[9] = psEnc->sCmn.la_shape; h[10] = psEnc->sCmn.warping_Q16;
        sd_i32("nsa.hdr", h, 11);
        sd_i32("nsa.quality", psEnc->sCmn.input_quality_bands_Q15, 4);
        sd_flt("nsa.predgain", psEncCtrl->predGain);
        sd_flt("nsa.ltpcorr", psEnc->LTPCorr);
        sd_flt("nsa.harm_smth", psShapeSt->HarmShapeGain_smth);
        sd_flt("nsa.tilt_smth", psShapeSt->Tilt_smth);
        sd_i32("nsa.pitchL", psEncCtrl->pitchL, psEnc->sCmn.nb_subfr);
        sd_f("nsa.x", x - psEnc->sCmn.la_shape, psEnc->sCmn.frame_length + 2 * psEnc->sCmn.la_shape);
        sd_f("nsa.pitchres", pitch_res, psEnc->sCmn.frame_length);
    }
    /* Point to start of first LPC analysis block */
    x_ptr = x - psEnc->sCmn.la_shape;"""),
    ("""        psShapeSt->Tilt_smth          += SUBFR_SMTH_COEF * ( Tilt - psShapeSt->Tilt_smth );
        psEncCtrl->Tilt[ k ]           = psShapeSt->Tilt_smth;
    }""", """        psShapeSt->Tilt_smth          += SUBFR_SMTH_COEF * ( Tilt - psShapeSt->Tilt_smth );
        psEncCtrl->Tilt[ k ]           = psShapeSt->Tilt_smth;
    }
    {
        int nb = psEnc->sCmn.nb_subfr;
        sd_flt("nsa.input_quality", psEncCtrl->input_quality);
        sd_flt("nsa.coding_quality", psEncCtrl->coding_quality);
        sd_int("nsa.qoffset", psEnc->sCmn.indices.quantOffsetType);
        sd_f("nsa.AR", psEncCtrl->AR, nb * MAX_SHAPE_LPC_ORDER);
        sd_f("nsa.gains", psEncCtrl->Gains, nb);
        sd_f("nsa.lfma", psEncCtrl->LF_MA_shp, nb);
        sd_f("nsa.lfar", psEncCtrl->LF_AR_shp, nb);
        sd_f("nsa.tilt", psEncCtrl->Tilt, nb);
        sd_f("nsa.harm", psEncCtrl->HarmShapeGain, nb);
        sd_flt("nsa.harm_smth_out", psShapeSt->HarmShapeGain_smth);
        sd_flt("nsa.tilt_smth_out", psShapeSt->Tilt_smth);
    }"""),
])
patch("silk/float/process_gains_FLP.c", [
    ("""    /* Gain reduction when LTP coding gain is high */""", """    {
        int h[10];
        h[0] = psEnc->sCmn.indices.signalType; h[1] = psEnc->sCmn.nb_subfr; h[2] = psEnc->sCmn.subfr_length;
        h[3] = psEnc->sCmn.SNR_dB_Q7; h[4] = psShapeSt->LastGainIndex; h[5] = condCoding;
        h[6] = psEnc->sCmn.input_tilt_Q15; h[7] = psEnc->sCmn.nStatesDelayedDecision;
        h[8] = psEnc->sCmn.speech_activity_Q8; h[9] = psEnc->sCmn.indices.quantOffsetType;
        sd_i32("pg.hdr", h, 10);
        sd_flt("pg.ltpred", psEncCtrl->LTPredCodGain);
        sd_flt("pg.input_quality", psEncCtrl->input_quality);
        sd_flt("pg.coding_quality", psEncCtrl->coding_quality);
        sd_f("pg.gains_in", psEncCtrl->Gains, psEnc->sCmn.nb_subfr);
        sd_f("pg.resnrg", psEncCtrl->ResNrg, psEnc->sCmn.nb_subfr);
    }
    /* Gain reduction when LTP coding gain is high */"""),
    ("""    silk_assert( psEncCtrl->Lambda > 0.0f );""", """    sd_f("pg.gains_out", psEncCtrl->Gains, psEnc->sCmn.nb_subfr);
    sd_i8("pg.gainidx", psEnc->sCmn.indices.GainsIndices, psEnc->sCmn.nb_subfr);
    sd_int("pg.lastgain", psShapeSt->LastGainIndex);
    sd_i32("pg.gainsunq", psEncCtrl->GainsUnq_Q16, psEnc->sCmn.nb_subfr);
    sd_int("pg.lastgainprev", psEncCtrl->lastGainIndexPrev);
    sd_int("pg.qoffset", psEnc->sCmn.indices.quantOffsetType);
    sd_flt("pg.lambda", psEncCtrl->Lambda);
    silk_assert( psEncCtrl->Lambda > 0.0f );"""),
])
# 2.11.2: the noise shaping quantiser, and the conversion in front of it.
NSQ_HASH = """    {
        unsigned long long h = 1469598103934665603ULL; int i;
        for (i = 0; i < 2 * MAX_FRAME_LENGTH; i++) h = (h ^ (unsigned int)(int)NSQ->xq[i]) * 1099511628211ULL;
        for (i = 0; i < 2 * MAX_FRAME_LENGTH; i++) h = (h ^ (unsigned int)NSQ->sLTP_shp_Q14[i]) * 1099511628211ULL;
        for (i = 0; i < MAX_SUB_FRAME_LENGTH + NSQ_LPC_BUF_LENGTH; i++) h = (h ^ (unsigned int)NSQ->sLPC_Q14[i]) * 1099511628211ULL;
        for (i = 0; i < MAX_SHAPE_LPC_ORDER; i++) h = (h ^ (unsigned int)NSQ->sAR2_Q14[i]) * 1099511628211ULL;
        h = (h ^ (unsigned int)NSQ->sLF_AR_shp_Q14) * 1099511628211ULL;
        h = (h ^ (unsigned int)NSQ->sDiff_shp_Q14) * 1099511628211ULL;
        h = (h ^ (unsigned int)NSQ->lagPrev) * 1099511628211ULL;
        h = (h ^ (unsigned int)NSQ->sLTP_buf_idx) * 1099511628211ULL;
        h = (h ^ (unsigned int)NSQ->sLTP_shp_buf_idx) * 1099511628211ULL;
        h = (h ^ (unsigned int)NSQ->rand_seed) * 1099511628211ULL;
        h = (h ^ (unsigned int)NSQ->prev_gain_Q16) * 1099511628211ULL;
        h = (h ^ (unsigned int)NSQ->rewhite_flag) * 1099511628211ULL;
        { int w[2]; w[0] = (int)(unsigned int)(h & 0xFFFFFFFFULL); w[1] = (int)(unsigned int)(h >> 32); sd_i32("%s", w, 2); }
    }
"""
patch("silk/NSQ.c", [
    ("""    SAVE_STACK;

    NSQ->rand_seed = psIndices->Seed;""", """    SAVE_STACK;
    {
        int hd[13], i, j, nb = psEncC->nb_subfr, so = psEncC->shapingLPCOrder;
        short ar[ MAX_NB_SUBFR * MAX_SHAPE_LPC_ORDER ];
        hd[0] = psEncC->ltp_mem_length; hd[1] = psEncC->frame_length; hd[2] = psEncC->subfr_length;
        hd[3] = nb; hd[4] = so; hd[5] = psEncC->predictLPCOrder; hd[6] = psIndices->Seed;
        hd[7] = psIndices->signalType; hd[8] = psIndices->quantOffsetType; hd[9] = psIndices->NLSFInterpCoef_Q2;
        hd[10] = Lambda_Q10; hd[11] = LTP_scale_Q14; hd[12] = psEncC->frameCounter;
        sd_i32("nsq.hdr", hd, 13);
        sd_i16("nsq.x16", x16, psEncC->frame_length);
        sd_i16("nsq.pred", PredCoef_Q12, 2 * MAX_LPC_ORDER);
        sd_i16("nsq.ltp", LTPCoef_Q14, nb * LTP_ORDER);
        for (i = 0; i < nb; i++) for (j = 0; j < so; j++) ar[i * so + j] = AR_Q13[i * MAX_SHAPE_LPC_ORDER + j];
        sd_i16("nsq.ar", ar, nb * so);
        sd_i32("nsq.harm", HarmShapeGain_Q14, nb); sd_i32("nsq.tilt", Tilt_Q14, nb);
        sd_i32("nsq.lf", LF_shp_Q14, nb); sd_i32("nsq.gains", Gains_Q16, nb); sd_i32("nsq.pitchL", pitchL, nb);
        {
            int sc[8];
            sd_i16("nsq.st_xq", NSQ->xq, 2 * MAX_FRAME_LENGTH);
            sd_i32("nsq.st_shp", NSQ->sLTP_shp_Q14, 2 * MAX_FRAME_LENGTH);
            sd_i32("nsq.st_lpc", NSQ->sLPC_Q14, MAX_SUB_FRAME_LENGTH + NSQ_LPC_BUF_LENGTH);
            sd_i32("nsq.st_ar2", NSQ->sAR2_Q14, MAX_SHAPE_LPC_ORDER);
            sc[0] = NSQ->sLF_AR_shp_Q14; sc[1] = NSQ->sDiff_shp_Q14; sc[2] = NSQ->lagPrev; sc[3] = NSQ->sLTP_buf_idx;
            sc[4] = NSQ->sLTP_shp_buf_idx; sc[5] = NSQ->rand_seed; sc[6] = NSQ->prev_gain_Q16; sc[7] = NSQ->rewhite_flag;
            sd_i32("nsq.st_sc", sc, 8);
        }
    }
""" + NSQ_HASH % "nsq.hash_in" + """    NSQ->rand_seed = psIndices->Seed;"""),
    ("""    silk_memmove( NSQ->sLTP_shp_Q14, &NSQ->sLTP_shp_Q14[ psEncC->frame_length ], psEncC->ltp_mem_length * sizeof( opus_int32 ) );
    RESTORE_STACK;""", """    silk_memmove( NSQ->sLTP_shp_Q14, &NSQ->sLTP_shp_Q14[ psEncC->frame_length ], psEncC->ltp_mem_length * sizeof( opus_int32 ) );
    sd_i8("nsq.pulses", pulses - psEncC->frame_length, psEncC->frame_length);
""" + NSQ_HASH % "nsq.hash_out" + """    RESTORE_STACK;"""),
])
# 2.12.3: the delayed-decision quantiser, recorded as the plain one is (its
# winning seed replaces the indices' at the end).
NSQ_IN = """    {
        int hd[13], i, j, nb = psEncC->nb_subfr, so = psEncC->shapingLPCOrder;
        short ar[ MAX_NB_SUBFR * MAX_SHAPE_LPC_ORDER ];
        hd[0] = psEncC->ltp_mem_length; hd[1] = psEncC->frame_length; hd[2] = psEncC->subfr_length;
        hd[3] = nb; hd[4] = so; hd[5] = psEncC->predictLPCOrder; hd[6] = psIndices->Seed;
        hd[7] = psIndices->signalType; hd[8] = psIndices->quantOffsetType; hd[9] = psIndices->NLSFInterpCoef_Q2;
        hd[10] = Lambda_Q10; hd[11] = LTP_scale_Q14; hd[12] = psEncC->frameCounter;
        sd_i32("nsq.hdr", hd, 13);
        { int dh[2]; dh[0] = psEncC->nStatesDelayedDecision; dh[1] = psEncC->warping_Q16; sd_i32("nsq.dd", dh, 2); }
        sd_i16("nsq.x16", x16, psEncC->frame_length);
        sd_i16("nsq.pred", PredCoef_Q12, 2 * MAX_LPC_ORDER);
        sd_i16("nsq.ltp", LTPCoef_Q14, nb * LTP_ORDER);
        for (i = 0; i < nb; i++) for (j = 0; j < so; j++) ar[i * so + j] = AR_Q13[i * MAX_SHAPE_LPC_ORDER + j];
        sd_i16("nsq.ar", ar, nb * so);
        sd_i32("nsq.harm", HarmShapeGain_Q14, nb); sd_i32("nsq.tilt", Tilt_Q14, nb);
        sd_i32("nsq.lf", LF_shp_Q14, nb); sd_i32("nsq.gains", Gains_Q16, nb); sd_i32("nsq.pitchL", pitchL, nb);
    }
"""
patch("silk/NSQ_del_dec.c", [
    ("""    /* Set unvoiced lag to the previous one, overwrite later for voiced */
    lag = NSQ->lagPrev;""", NSQ_IN + NSQ_HASH % "nsq.hash_in" + """    /* Set unvoiced lag to the previous one, overwrite later for voiced */
    lag = NSQ->lagPrev;"""),
    ("""    silk_memmove( NSQ->sLTP_shp_Q14, &NSQ->sLTP_shp_Q14[ psEncC->frame_length ], psEncC->ltp_mem_length * sizeof( opus_int32 ) );
    RESTORE_STACK;""", """    silk_memmove( NSQ->sLTP_shp_Q14, &NSQ->sLTP_shp_Q14[ psEncC->frame_length ], psEncC->ltp_mem_length * sizeof( opus_int32 ) );
    sd_i8("nsq.pulses", pulses - psEncC->frame_length, psEncC->frame_length);
    sd_int("nsq.seed_out", psIndices->Seed);
""" + NSQ_HASH % "nsq.hash_out" + """    RESTORE_STACK;"""),
])
# libopus runs the SSE4.1 build of it where the CPU has SSE4.1 (bit-exact with
# the C one, which OPUS_CHECK_ASM asserts): recorded the same way.
patch("silk/x86/NSQ_del_dec_sse4_1.c", [
    ("""    /* Set unvoiced lag to the previous one, overwrite later for voiced */
    lag = NSQ->lagPrev;""", NSQ_IN + NSQ_HASH % "nsq.hash_in" + """    /* Set unvoiced lag to the previous one, overwrite later for voiced */
    lag = NSQ->lagPrev;"""),
    ("""    silk_memmove( NSQ->sLTP_shp_Q14, &NSQ->sLTP_shp_Q14[ psEncC->frame_length ], psEncC->ltp_mem_length * sizeof( opus_int32 ) );
""", """    silk_memmove( NSQ->sLTP_shp_Q14, &NSQ->sLTP_shp_Q14[ psEncC->frame_length ], psEncC->ltp_mem_length * sizeof( opus_int32 ) );
    sd_i8("nsq.pulses", pulses - psEncC->frame_length, psEncC->frame_length);
    sd_int("nsq.seed_out", psIndices->Seed);
""" + NSQ_HASH % "nsq.hash_out"),
])
# and the AVX2 build where the CPU has AVX2 (falling back to the C one for
# settings it does not handle)
patch("silk/x86/NSQ_del_dec_avx2.c", [
    ("""    /* Set unvoiced lag to the previous one, overwrite later for voiced */
    lag = NSQ->lagPrev;""", NSQ_IN + NSQ_HASH % "nsq.hash_in" + """    /* Set unvoiced lag to the previous one, overwrite later for voiced */
    lag = NSQ->lagPrev;"""),
    ("""    silk_memmove(NSQ->sLTP_shp_Q14, &NSQ->sLTP_shp_Q14[psEncC->frame_length], psEncC->ltp_mem_length * sizeof(opus_int32));
""", """    silk_memmove(NSQ->sLTP_shp_Q14, &NSQ->sLTP_shp_Q14[psEncC->frame_length], psEncC->ltp_mem_length * sizeof(opus_int32));
    sd_i8("nsq.pulses", pulses - psEncC->frame_length, psEncC->frame_length);
    sd_int("nsq.seed_out", psIndices->Seed);
""" + NSQ_HASH % "nsq.hash_out"),
])
patch("silk/float/wrappers_FLP.c", [
    ("""    /* Convert control struct to fix control struct */""", """    {
        int hd[6], i, j, nb = psEnc->sCmn.nb_subfr, so = psEnc->sCmn.shapingLPCOrder, po = psEnc->sCmn.predictLPCOrder;
        float ar[ MAX_NB_SUBFR * MAX_SHAPE_LPC_ORDER ], pc[ 2 * MAX_LPC_ORDER ];
        hd[0] = nb; hd[1] = so; hd[2] = po; hd[3] = psEnc->sCmn.frame_length; hd[4] = psIndices->signalType;
        hd[5] = psIndices->LTP_scaleIndex;
        sd_i32("nsw.hdr", hd, 6);
        for (i = 0; i < nb; i++) for (j = 0; j < so; j++) ar[i * so + j] = psEncCtrl->AR[i * MAX_SHAPE_LPC_ORDER + j];
        sd_f("nsw.ar", ar, nb * so);
        sd_f("nsw.lfar", psEncCtrl->LF_AR_shp, nb); sd_f("nsw.lfma", psEncCtrl->LF_MA_shp, nb);
        sd_f("nsw.tilt", psEncCtrl->Tilt, nb); sd_f("nsw.harm", psEncCtrl->HarmShapeGain, nb);
        sd_flt("nsw.lambda", psEncCtrl->Lambda);
        sd_f("nsw.ltp", psEncCtrl->LTPCoef, nb * LTP_ORDER);
        for (j = 0; j < 2; j++) for (i = 0; i < po; i++) pc[j * po + i] = psEncCtrl->PredCoef[j][i];
        sd_f("nsw.pred", pc, 2 * po);
        sd_f("nsw.gains", psEncCtrl->Gains, nb);
        sd_f("nsw.x", x, psEnc->sCmn.frame_length);
    }
    /* Convert control struct to fix control struct */"""),
])
# 2.12.0: the voiced path. The voicing decision and the pitch estimator (its
# 8 and 4 kHz decimations, its first-stage candidates and second-stage lag);
# the LTP analysis, quantisation and scaling; the pitch-tracking high-pass.
patch("silk/float/find_pitch_lags_FLP.c", [
    ("""        /*****************************************/
        /* Call Pitch estimator                  */
        /*****************************************/""", """        {
            int h[12];
            h[0] = psEnc->sCmn.prevLag; h[1] = psEnc->sCmn.fs_kHz; h[2] = psEnc->sCmn.pitchEstimationComplexity;
            h[3] = psEnc->sCmn.nb_subfr; h[4] = psEnc->sCmn.indices.signalType; h[5] = psEnc->sCmn.prevSignalType;
            h[6] = psEnc->sCmn.speech_activity_Q8; h[7] = psEnc->sCmn.input_tilt_Q15;
            h[8] = psEnc->sCmn.pitchEstimationLPCOrder; h[9] = psEnc->sCmn.pitchEstimationThreshold_Q16;
            h[10] = psEnc->sCmn.frameCounter; h[11] = psEnc->sCmn.ltp_mem_length;
            sd_i32("pac.hdr", h, 12);
            sd_flt("pac.thr", thrhld);
            sd_flt("pac.ltpcorr_in", psEnc->LTPCorr);
        }
        /*****************************************/
        /* Call Pitch estimator                  */
        /*****************************************/"""),
    ("""            psEnc->sCmn.indices.signalType = TYPE_UNVOICED;
        }""", """            psEnc->sCmn.indices.signalType = TYPE_UNVOICED;
        }
        {
            int o[7], k;
            o[0] = psEnc->sCmn.indices.signalType; o[1] = psEnc->sCmn.indices.lagIndex;
            o[2] = psEnc->sCmn.indices.contourIndex;
            for (k = 0; k < 4; k++) o[3 + k] = psEncCtrl->pitchL[k];
            sd_i32("pac.out", o, 7);
            sd_flt("pac.ltpcorr", psEnc->LTPCorr);
        }"""),
])
patch("silk/float/pitch_analysis_core_FLP.c", [
    ("""    silk_resampler_down2( filt_state, frame_4_FIX, frame_8_FIX, frame_length_8kHz );""",
     """    silk_resampler_down2( filt_state, frame_4_FIX, frame_8_FIX, frame_length_8kHz );
    sd_i16("pac.f8", frame_8_FIX, frame_length_8kHz);
    sd_i16("pac.f4", frame_4_FIX, frame_length_4kHz);"""),
    ("""    celt_assert( length_d_srch > 0 );""", """    celt_assert( length_d_srch > 0 );
    { int t[1]; t[0] = length_d_srch; sd_i32("pac.nsrch", t, 1); sd_i32("pac.srch", d_srch, length_d_srch); }"""),
    ("""    *LTPCorr = (silk_float)( CCmax / nb_subfr );""", """    *LTPCorr = (silk_float)( CCmax / nb_subfr );
    { int t[2]; t[0] = lag; t[1] = CBimax; sd_i32("pac.st2", t, 2); }"""),
])
patch("silk/float/find_pred_coefs_FLP.c", [
    ("""        /* Quantize LTP gain parameters */""", """        {
            int h[14], k, nb = psEnc->sCmn.nb_subfr;
            h[0] = nb; h[1] = psEnc->sCmn.subfr_length; h[2] = psEnc->sCmn.predictLPCOrder; h[3] = condCoding;
            h[4] = psEnc->sCmn.sum_log_gain_Q7; h[5] = psEnc->sCmn.PacketLoss_perc;
            h[6] = psEnc->sCmn.nFramesPerPacket; h[7] = psEnc->sCmn.LBRR_flag; h[8] = psEnc->sCmn.SNR_dB_Q7;
            h[9] = psEnc->sCmn.ltp_mem_length;
            for (k = 0; k < 4; k++) h[10 + k] = psEncCtrl->pitchL[k];
            sd_i32("ltp.hdr", h, 14);
            sd_f("ltp.res", res_pitch - psEnc->sCmn.ltp_mem_length,
                psEnc->sCmn.ltp_mem_length + psEnc->sCmn.frame_length + psEnc->sCmn.la_pitch);
            sd_f("ltp.x", x - psEnc->sCmn.ltp_mem_length, psEnc->sCmn.ltp_mem_length + psEnc->sCmn.frame_length);
            sd_f("ltp.XX", XXLTP, nb * LTP_ORDER * LTP_ORDER);
            sd_f("ltp.xX", xXLTP, nb * LTP_ORDER);
        }
        /* Quantize LTP gain parameters */"""),
    ("""        /* Control LTP scaling */""", """        {
            int o[6], k, nb = psEnc->sCmn.nb_subfr;
            o[0] = psEnc->sCmn.indices.PERIndex;
            for (k = 0; k < 4; k++) o[1 + k] = k < nb ? psEnc->sCmn.indices.LTPIndex[k] : 0;
            o[5] = psEnc->sCmn.sum_log_gain_Q7;
            sd_i32("ltp.idx", o, 6);
            sd_f("ltp.B", psEncCtrl->LTPCoef, nb * LTP_ORDER);
            sd_flt("ltp.predgain", psEncCtrl->LTPredCodGain);
        }
        /* Control LTP scaling */"""),
    ("""        /* Create LTP residual */""", """        sd_int("ltp.scale", psEnc->sCmn.indices.LTP_scaleIndex);
        sd_flt("ltp.scalef", psEncCtrl->LTP_scale);
        /* Create LTP residual */"""),
])
patch("silk/enc_API.c", [
    ("""            silk_HP_variable_cutoff( psEnc->state_Fxx );""", """            {
                int h[6]; silk_encoder_state *c1 = &psEnc->state_Fxx[ 0 ].sCmn;
                h[0] = c1->prevSignalType; h[1] = c1->prevLag; h[2] = c1->fs_kHz; h[3] = c1->input_quality_bands_Q15[0];
                h[4] = c1->speech_activity_Q8; h[5] = c1->variable_HP_smth1_Q15;
                sd_i32("hp.in", h, 6);
            }
            silk_HP_variable_cutoff( psEnc->state_Fxx );
            sd_int("hp.out", psEnc->state_Fxx[ 0 ].sCmn.variable_HP_smth1_Q15);"""),
])
# 2.12.2: stereo, silk_stereo_LR_to_MS's inputs, state and outputs.
MS_STATE = """    {
        int t[13], k;
        t[0] = state->pred_prev_Q13[0]; t[1] = state->pred_prev_Q13[1]; t[2] = state->sMid[0]; t[3] = state->sMid[1];
        t[4] = state->sSide[0]; t[5] = state->sSide[1];
        for (k = 0; k < 4; k++) t[6 + k] = state->mid_side_amp_Q0[k];
        t[10] = state->smth_width_Q14; t[11] = state->width_prev_Q14; t[12] = state->silent_side_len;
        sd_i32("%s", t, 13);
    }
"""
patch("silk/stereo_LR_to_MS.c", [
    ("""    opus_int16 *mid = &x1[ -2 ];
    SAVE_STACK;
""", """    opus_int16 *mid = &x1[ -2 ];
    SAVE_STACK;
    {
        int h[6];
        h[0] = total_rate_bps; h[1] = prev_speech_act_Q8; h[2] = toMono; h[3] = fs_kHz; h[4] = frame_length;
        h[5] = 0;
        sd_i32("ms.hdr", h, 6);
        sd_i16("ms.x1", x1 - 2, frame_length + 2);
        sd_i16("ms.x2", x2 - 2, frame_length + 2);
    }
""" + MS_STATE % "ms.st_in"),
    ("""    frac_Q16 = silk_SMLABB( HP_ratio_Q14, LP_ratio_Q14, 3 );""", """    { int t[4]; t[0] = pred_Q13[0]; t[1] = pred_Q13[1]; t[2] = LP_ratio_Q14; t[3] = HP_ratio_Q14; sd_i32("ms.pred", t, 4); }
    frac_Q16 = silk_SMLABB( HP_ratio_Q14, LP_ratio_Q14, 3 );"""),
    ("""    state->width_prev_Q14     = (opus_int16)width_Q14;
    RESTORE_STACK;""", """    state->width_prev_Q14     = (opus_int16)width_Q14;
    {
        int o[9], k;
        for (k = 0; k < 6; k++) o[k] = ix[k / 3][k % 3];
        o[6] = *mid_only_flag; o[7] = mid_side_rates_bps[0]; o[8] = mid_side_rates_bps[1];
        sd_i32("ms.out", o, 9);
        sd_i16("ms.mid", mid, frame_length + 2);
        sd_i16("ms.side", x2 - 1, frame_length);
    }
""" + MS_STATE % "ms.st_out" + """    RESTORE_STACK;"""),
])


# 2.12.4: the CELT encoder, celt_encode_with_ec, at each stage boundary
# (celtdump.h, $CELTDUMP). Floats as bit patterns, so the comparison is exact.
shutil.copy(os.path.join(HERE, "celtdump.h"), os.path.join(ROOT, "celt", "celtdump.h"))


def patch_celt(rel, pairs):
    path = os.path.join(ROOT, rel)
    s = open(path).read()
    if '#include "celtdump.h"' not in s:
        anchor = '#include "celt_lpc.h"' if '#include "celt_lpc.h"' in s else '#include "vq.h"'
        if s.count(anchor) < 1:
            sys.exit("patch: no include anchor in " + rel)
        i = s.index(anchor)
        s = s[:i] + '#include "celtdump.h"\n' + s[i:]
    for old, new in pairs:
        if s.count(old) != 1:
            sys.exit("patch: anchor not found once in %s: %r" % (rel, old[:60]))
        s = s.replace(old, new)
    open(path, "w").write(s)


patch_celt("celt/celt_encoder.c", [
    # transient_analysis's per-channel metric
    ("""      /*for (i=0;i<len2;i++)printf("%f ", tmp[i]/mean);printf("\\n");*/""",
     """      cd_f("ta.tmp", tmp, len2);"""),
    ("""      unmask = 64*unmask*4/(6*(len2-17));""",
     """      unmask = 64*unmask*4/(6*(len2-17));
      { int t[2]; float f[3]; t[0] = c; t[1] = unmask; cd_i("ta.unmask", t, 2); f[0] = mean; f[1] = norm; f[2] = maxE; cd_f("ta.mean", f, 3); }"""),
    # the frame's shape and budget, the silence decision, the pre-emphasised input
    ("""   tone_freq = tone_detect(in, CC, N+overlap, &toneishness, mode->Fs);""",
     """   { int h[12]; h[0] = LM; h[1] = C; h[2] = CC; h[3] = start; h[4] = end; h[5] = tell; h[6] = nbCompressedBytes;
     h[7] = nbAvailableBytes; h[8] = effectiveBytes; h[9] = vbr_rate; h[10] = equiv_rate; h[11] = silence; cd_i("ce.begin", h, 12);
     cd_flt("ce.smax", sample_max); cd_f("ce.pre", in, CC*(N+overlap)); }
   tone_freq = tone_detect(in, CC, N+overlap, &toneishness, mode->Fs);
   { float t[2]; t[0] = tone_freq; t[1] = toneishness; cd_f("ce.tone", t, 2); }"""),
    ("""   toneishness = MIN32(toneishness, QCONST32(1.f, 29)-SHL32(tf_estimate, 15));""",
     """   { int t[3]; float f[2]; t[0] = isTransient; t[1] = tf_chan; t[2] = weak_transient; cd_i("ce.trans", t, 3);
     f[0] = tf_estimate; f[1] = toneishness; cd_f("ce.tfest", f, 2); }
   toneishness = MIN32(toneishness, QCONST32(1.f, 29)-SHL32(tf_estimate, 15));"""),
    ("""      if ((gain1 > QCONST16(.4f,15) || st->prefilter_gain > QCONST16(.4f,15)) && (!st->analysis.valid || st->analysis.tonality > .3)""",
     """      { int t[4]; t[0] = pf_on; t[1] = pitch_index; t[2] = qg; t[3] = prefilter_tapset; cd_i("ce.pf", t, 4);
        cd_flt("ce.pfgain", gain1); cd_f("ce.inpf", in, CC*(N+overlap)); }
      if ((gain1 > QCONST16(.4f,15) || st->prefilter_gain > QCONST16(.4f,15)) && (!st->analysis.valid || st->analysis.tonality > .3)"""),
    ("""   amp2Log2(mode, effEnd, end, bandE, bandLogE, C);

   ALLOC(surround_dynalloc, C*nbEBands, celt_glog);""",
     """   amp2Log2(mode, effEnd, end, bandE, bandLogE, C);
   { int t[3]; t[0] = isTransient; t[1] = shortBlocks; t[2] = secondMdct; cd_i("ce.mdct", t, 3);
     cd_f("ce.freq", freq, C*N); cd_f("ce.bandE", bandE, nbEBands*C); cd_f("ce.logE", bandLogE, nbEBands*C);
     if (secondMdct) cd_f("ce.logE2", bandLogE2, nbEBands*C); }

   ALLOC(surround_dynalloc, C*nbEBands, celt_glog);"""),
    ("""   if (!secondMdct)
   {
      OPUS_COPY(bandLogE2, bandLogE, C*nbEBands);
   }""",
     """   { float t[4]; t[0] = temporal_vbr; t[1] = st->spec_avg; t[2] = surround_masking; t[3] = surround_trim; cd_f("ce.tvbr", t, 4);
     cd_f("ce.sdyn", surround_dynalloc, end); }
   if (!secondMdct)
   {
      OPUS_COPY(bandLogE2, bandLogE, C*nbEBands);
   }"""),
    ("""   ALLOC(X, C*N, celt_norm);         /**< Interleaved normalised MDCTs */""",
     """   { int t[2]; t[0] = isTransient; t[1] = shortBlocks; cd_i("ce.trans2", t, 2);
     cd_f("ce.logEp", bandLogE, nbEBands*C); cd_f("ce.logE2p", bandLogE2, nbEBands*C); cd_flt("ce.tfest2", tf_estimate); }
   ALLOC(X, C*N, celt_norm);         /**< Interleaved normalised MDCTs */"""),
    ("""   ALLOC(tf_res, nbEBands, int);""",
     """   cd_f("ce.X", X, C*N);
   { int t[2]; t[0] = tot_boost; t[1] = enable_tf_analysis; cd_i("ce.dyn", t, 2); cd_flt("ce.maxdepth", maxDepth);
     cd_i("ce.offsets", offsets, nbEBands); cd_i("ce.importance", importance + start, end - start);
     cd_i("ce.spreadw", spread_weight, end); }
   ALLOC(tf_res, nbEBands, int);"""),
    ("""   ALLOC(error, C*nbEBands, celt_glog);""",
     """   cd_int("ce.tfsel", tf_select); cd_i("ce.tfres", tf_res, end);
   ALLOC(error, C*nbEBands, celt_glog);"""),
    ("""   tf_encode(start, end, isTransient, tf_res, LM, tf_select, enc);""",
     """   cd_f("ce.logEq", bandLogE, nbEBands*C); cd_f("ce.coarse", oldBandE, nbEBands*C); cd_f("ce.error", error, nbEBands*C);
   cd_flt("ce.dintra", st->delayedIntra);
   tf_encode(start, end, isTransient, tf_res, LM, tf_select, enc);
   cd_i("ce.tfcode", tf_res, end);"""),
    ("""   /* For LFE, everything interesting is in the first band */""",
     """   { int t[4]; t[0] = st->spread_decision; t[1] = st->tapset_decision; t[2] = st->tonal_average; t[3] = st->hf_average; cd_i("ce.spread", t, 4); }
   /* For LFE, everything interesting is in the first band */"""),
    ("""   alloc_trim = 5;
   if (tell+(6<<BITRES) <= total_bits - total_boost)""",
     """   cd_i("ce.boost", offsets, nbEBands);
   { int t[3]; t[0] = total_boost; t[1] = dual_stereo; t[2] = st->intensity; cd_i("ce.stereo", t, 3); }
   alloc_trim = 5;
   if (tell+(6<<BITRES) <= total_bits - total_boost)"""),
    ("""   /* In VBR mode the frame size must not be reduced so much that it would
       result in the encoder running out of bits.""",
     """   cd_int("ce.trim", alloc_trim); cd_flt("ce.ssave", st->stereo_saving);
   /* In VBR mode the frame size must not be reduced so much that it would
       result in the encoder running out of bits."""),
    ("""   /* Bit allocation */
   ALLOC(fine_quant, nbEBands, int);""",
     """   { int t[7]; t[0] = nbCompressedBytes; t[1] = nbAvailableBytes; t[2] = st->vbr_reservoir; t[3] = st->vbr_drift;
     t[4] = st->vbr_offset; t[5] = st->vbr_count; t[6] = min_allowed; cd_i("ce.vbr", t, 7); }
   /* Bit allocation */
   ALLOC(fine_quant, nbEBands, int);"""),
    ("""   quant_fine_energy(mode, start, end, oldBandE, error, NULL, fine_quant, enc, C);""",
     """   { int t[7]; t[0] = codedBands; t[1] = balance; t[2] = st->intensity; t[3] = dual_stereo; t[4] = bits;
     t[5] = anti_collapse_rsv; t[6] = signalBandwidth; cd_i("ce.alloc", t, 7);
     cd_i("ce.pulses", pulses, nbEBands); cd_i("ce.fineq", fine_quant, nbEBands); cd_i("ce.finep", fine_priority, nbEBands); }
   quant_fine_energy(mode, start, end, oldBandE, error, NULL, fine_quant, enc, C);
   cd_f("ce.fine", oldBandE, nbEBands*C); cd_f("ce.ferror", error, nbEBands*C);"""),
    ("""   if (anti_collapse_rsv > 0)
   {
      anti_collapse_on = st->consec_transient<2;""",
     """   cd_u8("ce.cm", collapse_masks, C*nbEBands); { unsigned int t = st->rng; cd_i("ce.qrng", (int *)&t, 1); }
   if (anti_collapse_rsv > 0)
   {
      anti_collapse_on = st->consec_transient<2;"""),
    ("""   if (silence)
   {
      for (i=0;i<C*nbEBands;i++)
         oldBandE[i] = -GCONST(28.f);
   }
""",
     """   cd_f("ce.final", oldBandE, nbEBands*C); cd_f("ce.eerr", energyError, nbEBands*CC);
   if (silence)
   {
      for (i=0;i<C*nbEBands;i++)
         oldBandE[i] = -GCONST(28.f);
   }
"""),
    ("""   st->rng = enc->rng;

   /* If there's any room left (can only happen for very high rates),""",
     """   st->rng = enc->rng;
   { int t[3]; t[0] = (int)enc->rng; t[1] = nbCompressedBytes; t[2] = st->consec_transient; cd_i("ce.end", t, 3); }

   /* If there's any room left (can only happen for very high rates),"""),
])

print("patch: instrumented " + ROOT)
