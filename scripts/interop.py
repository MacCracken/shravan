#!/usr/bin/env python3
"""Sweep shravan against reference decoders and encoders, in both directions.

The in-suite fixtures (src/interop_fixtures.cyr) pin a few dozen streams. This sweep
crosses the matrix: sample rate x bit depth x channel count for every format, real
encoder output decoded by shravan and judged against ffmpeg, flac, mpg123 or libopus,
and shravan's encoders judged by what those references make of their output.

    ./scripts/interop.sh            # build the tools, run everything
    ./scripts/interop.py --only flac,opus

Lossless paths must match exactly, and WAV, AIFF and FLAC must match again through
the streaming decoders, fed in odd-sized pieces. Lossy decoders are judged by the maximum sample
error in 16-bit LSB against the reference (float rounding is well under 1 LSB);
lossy encoders by the correlation of the reference's decode with the input.

Needs ffmpeg (libmp3lame, libopus), flac, lame, twolame, mpg123, and
build/interop-cli + build/opusref (scripts/interop.sh builds both).
"""
import argparse
import array
import math
import os
import struct
import shutil
import subprocess
import sys
import tempfile
import wave

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CLI = os.path.join(ROOT, "build", "interop-cli")
OPUSREF = os.path.join(ROOT, "build", "opusref")
OPUSENCREF = os.path.join(ROOT, "build", "opusenc-ref")

# Bursts at different times per channel, a sweep and noise: transients force short
# blocks and independent windows, noise keeps every band busy.
def signal(rate, ch, dur, amp=0.4, sweep=None):
    """sweep: the chirp's rise per second (default rate / 5)."""
    chans = []
    for c in range(ch):
        f = 220 * (c + 2)
        chans.append(
            f"{amp}*sin(2*PI*{f}*t)*(0.6+0.4*exp(-20*mod(t+{0.03 * c:.2f}\\,0.17)))"
            f"+0.08*sin(2*PI*(300+{rate // 5 if sweep is None else sweep}*mod(t\\,1))*t)+0.04*random({c})")
    return "aevalsrc=" + "|".join(chans) + f":s={rate}:d={dur}"


# ffmpeg's ff_vorbis_channel_layout_offsets: Vorbis channel k is ffmpeg's offs[k].
VORBIS_OFFSETS = {3: (0, 2, 1), 4: (0, 1, 2, 3), 5: (0, 2, 1, 3, 4), 6: (0, 2, 1, 5, 3, 4),
                  7: (0, 2, 1, 6, 5, 3, 4), 8: (0, 2, 1, 7, 5, 6, 3, 4)}


def libopus_mapping_order(ref, ch):
    """ffmpeg's libopus wrapper reads every 3- to 8-channel stream as Vorbis order
    and reorders it to ffmpeg's, whatever its mapping family; RFC 7845 and ffmpeg's
    own Opus decoder (its default) reorder family 1 only, as shravan does. For
    families 2 and 255 this puts the wrapper's decode back in mapping order: its
    channel k is mapping-order channel offs[k]."""
    offs = VORBIS_OFFSETS.get(ch)
    if offs is None:
        return ref
    out = array.array("d", ref)
    for i in range(len(ref) // ch):
        for k in range(ch):
            out[i * ch + offs[k]] = ref[i * ch + k]
    return out


def run(*args, check=True):
    return subprocess.run(args, capture_output=True, check=check)


def f64(path):
    a = array.array("d")
    with open(path, "rb") as fh:
        raw = fh.read()
    a.frombytes(raw[: len(raw) // 8 * 8])
    return a


def ff_decode(src, dst, *pre, post=()):
    run("ffmpeg", "-v", "error", "-y", *pre, "-i", src, *post, "-f", "f64le", "-c:a", "pcm_f64le", dst)
    return f64(dst)


def mp4_audio_tracks(path):
    """The audio tracks of an MP4 (ISO 14496-12), in file order: for each, the
    tkhd enabled flag, the mdhd timescale, the edit list (segment duration in the
    movie's timescale, media time, rate) and the sum of the stts durations."""
    d = open(path, "rb").read()

    def boxes(s, e):
        p = s
        while p + 8 <= e:
            size, t = struct.unpack(">I4s", d[p:p + 8])
            hdr = 8
            if size == 1:
                size = struct.unpack(">Q", d[p + 8:p + 16])[0]
                hdr = 16
            if size == 0:
                size = e - p
            yield t, p + hdr, p + size
            p += size

    def child(s, e, path):
        for t, bs, be in boxes(s, e):
            if t == path[0]:
                return (bs, be) if len(path) == 1 else child(bs, be, path[1:])
        return None

    moov = child(0, len(d), [b"moov"])
    mvhd = child(*moov, [b"mvhd"])
    movie_ts = struct.unpack(">I", d[mvhd[0] + (20 if d[mvhd[0]] == 1 else 12):][:4])[0]
    tracks = []
    for t, ts_, te in boxes(*moov):
        if t != b"trak":
            continue
        hdlr = child(ts_, te, [b"mdia", b"hdlr"])
        if d[hdlr[0] + 8:hdlr[0] + 12] != b"soun":
            continue
        tkhd = child(ts_, te, [b"tkhd"])
        mdhd = child(ts_, te, [b"mdia", b"mdhd"])
        ts = struct.unpack(">I", d[mdhd[0] + (20 if d[mdhd[0]] == 1 else 12):][:4])[0]
        stts = child(ts_, te, [b"mdia", b"minf", b"stbl", b"stts"])
        n = struct.unpack(">I", d[stts[0] + 4:stts[0] + 8])[0]
        total = sum(c * dl for c, dl in (struct.unpack(">II", d[stts[0] + 8 + i * 8:stts[0] + 16 + i * 8])
                                         for i in range(n)))
        edits = []
        elst = child(ts_, te, [b"edts", b"elst"])
        if elst:
            v = d[elst[0]]
            for i in range(struct.unpack(">I", d[elst[0] + 4:elst[0] + 8])[0]):
                q = elst[0] + 8 + i * (20 if v == 1 else 12)
                dur, mt, rate = struct.unpack(">QqI" if v == 1 else ">IiI", d[q:q + (20 if v == 1 else 12)])
                edits.append((dur, mt, rate >> 16))
        tracks.append({"enabled": d[tkhd[0] + 3] & 1, "ts": ts, "edits": edits, "stts": total})
    return movie_ts, tracks


def mp4_iso_ref(path, dst, index, ch):
    """Track `index` as ISO 14496-12 presents it: ffmpeg's decode without the edit
    list (every ffmpeg gives the same), cut to the sample table's length, then to
    each edit's media range in order (empty and dwell edits give nothing). ffmpeg
    itself trims an MP4's last frame to the edit list only from 9.0; older ones
    (Ubuntu 24.04 has 6.1) play the padding, so this cut is the reference, and
    with ffmpeg 9 it equals ffmpeg's own default decode."""
    movie_ts, tracks = mp4_audio_tracks(path)
    tk = tracks[index]
    raw = ff_decode(path, dst, "-ignore_editlist", "1", post=("-map", f"0:a:{index}"))
    media = min(len(raw) // ch, tk["stts"])
    ranges = []
    for dur, mt, rate in tk["edits"]:
        if mt >= 0 and dur > 0 and rate != 0 and mt < media:
            ranges.append((mt, min((dur * tk["ts"] + movie_ts // 2) // movie_ts, media - mt)))
    if not tk["edits"]:
        ranges = [(0, media)]
    out = array.array("d")
    for s0, n0 in ranges:
        out.extend(raw[s0 * ch:(s0 + n0) * ch])
    return out


def ffmpeg_default_track(path, channels):
    """ffmpeg's default audio stream: enabled tracks first, then the most channels,
    then the first (fftools/ffmpeg_mux_init.c, map_auto_audio)."""
    _, tracks = mp4_audio_tracks(path)
    scores = [channels[i] + 5000000 * tk["enabled"] for i, tk in enumerate(tracks)]
    return scores.index(max(scores))


def shravan_decode(src, dst):
    p = run(CLI, "decode", src, dst, check=False)
    if p.returncode != 0:
        return None, p.stdout.decode().strip()
    return f64(dst), p.stdout.decode().split()


def shravan_stream(kind, src, dst, feed):
    p = run(CLI, "stream", kind, src, dst, str(feed), check=False)
    if p.returncode != 0:
        return None, p.stdout.decode().strip()
    return f64(dst), p.stdout.decode().split()


def maxerr(a, b, scale=32768.0):
    n = min(len(a), len(b))
    return max((abs(a[i] - b[i]) for i in range(n)), default=0.0) * scale


def corr(a, b):
    n = min(len(a), len(b))
    sab = sum(a[i] * b[i] for i in range(n))
    saa = sum(a[i] * a[i] for i in range(n))
    sbb = sum(b[i] * b[i] for i in range(n))
    return sab / math.sqrt(saa * sbb) if saa > 0 and sbb > 0 else 0.0


def best_lag_corr(ref, got, ch, maxlag=4096, warm=2048, minlag=None):
    """Correlation of got against ref at the lag (in frames, either direction, or
    minlag..maxlag) that maximises it, past the first and last `warm` frames (codec
    start-up)."""
    best = (0.0, 0)
    for lag in range(-maxlag if minlag is None else minlag, maxlag + 1):
        a, b = (ref, got[lag * ch:]) if lag >= 0 else (ref[-lag * ch:], got)
        n = min(len(a), len(b)) - warm * ch
        if n <= warm * ch:
            continue
        c = corr(a[warm * ch:n], b[warm * ch:n])
        if c > best[0]:
            best = (c, lag)
    return best


class Sweep:
    def __init__(self, tmp, verbose):
        self.tmp, self.verbose = tmp, verbose
        self.results = []

    def p(self, name):
        return os.path.join(self.tmp, name)

    def record(self, group, name, ok, detail):
        self.results.append((group, name, ok, detail))
        if self.verbose or not ok:
            print(f"  {'ok  ' if ok else 'FAIL'} {group:6s} {name:44s} {detail}")

    # -- decode direction -------------------------------------------------------

    def decode_vs(self, group, name, path, ref, lossless, tol=2.0, ref_scale=32768.0):
        got, info = shravan_decode(path, self.p("got.f64"))
        if got is None:
            self.record(group, name, False, f"shravan error: {info}")
            return
        e = maxerr(ref, got, ref_scale)
        same_len = len(ref) == len(got)
        ok = same_len and (e == 0 if lossless else e <= tol)
        self.record(group, name, ok, f"n={len(got)}/{len(ref)} maxerr={e:.3f}")
        if group in ("wav", "aiff", "flac") and lossless:
            # The same file through the streaming decoder, split at an odd size so
            # pieces end mid-sample and mid-frame. Before 2.9.3 a WAV or AIFF stream
            # returned only the first piece's samples.
            got, info = shravan_stream(group, path, self.p("st.f64"), 1009)
            if got is None:
                self.record(group, name + " (stream)", False, f"shravan error: {info}")
                return
            e = maxerr(ref, got, ref_scale)
            ok = len(ref) == len(got) and e == 0
            self.record(group, name + " (stream)", ok, f"n={len(got)}/{len(ref)} maxerr={e:.3f}")

    def wav(self):
        fmts = [("u8", "pcm_u8", 8), ("s16", "pcm_s16le", 16), ("s24", "pcm_s24le", 24),
                ("s32", "pcm_s32le", 32), ("f32", "pcm_f32le", 0), ("f64", "pcm_f64le", 0)]
        for rate in (8000, 44100, 96000, 192000):
            for ch in (1, 2, 6):
                for tag, codec, bits in fmts:
                    src = self.p(f"w.wav")
                    run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.1),
                        "-c:a", codec, src)
                    ref = ff_decode(src, self.p("ref.f64"))
                    self.decode_vs("wav", f"{rate} {ch}ch {tag}", src, ref, True,
                                   ref_scale=float(1 << (bits - 1)) if bits else 1 << 52)

    def aiff(self):
        fmts = [("s8", "pcm_s8"), ("s16", "pcm_s16be"), ("s24", "pcm_s24be"), ("s32", "pcm_s32be"),
                ("f32/aifc", "pcm_f32be"), ("f64/aifc", "pcm_f64be"), ("sowt/aifc", "pcm_s16le")]
        for rate in (22050, 48000, 96000):
            for ch in (1, 2, 6):
                for tag, codec in fmts:
                    src = self.p("a.aiff")
                    p = run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.1),
                            "-c:a", codec, src, check=False)
                    if p.returncode != 0:
                        continue
                    ref = ff_decode(src, self.p("ref.f64"))
                    self.decode_vs("aiff", f"{rate} {ch}ch {tag}", src, ref, True, ref_scale=1 << 31)

    def flac(self):
        for bits in (8, 16, 24, 32):
            for ch in (1, 2, 3, 6, 8):
                for opts in (("-0",), ("-8",), ("-8", "--lax", "-b", "16384"), ("-5", "-b", "192")):
                    if bits == 32 and "16384" in opts:
                        continue
                    wav = self.p("in.wav")
                    codec = {8: "pcm_u8", 16: "pcm_s16le", 24: "pcm_s24le", 32: "pcm_s32le"}[bits]
                    run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(48000, ch, 0.15),
                        "-c:a", codec, wav)
                    out = self.p("o.flac")
                    p = run("flac", "-s", "-f", *opts, wav, "-o", out, check=False)
                    if p.returncode != 0:
                        self.record("flac", f"{bits}b {ch}ch {' '.join(opts)}", False, "flac refused")
                        continue
                    ref = ff_decode(out, self.p("ref.f64"))
                    self.decode_vs("flac", f"{bits}b {ch}ch {' '.join(opts)}", out, ref, True,
                                   ref_scale=float(1 << (bits - 1)))
        # Wasted bits: 16-bit content in a 24-bit container.
        wav = self.p("in.wav")
        run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(44100, 2, 0.2),
            "-af", "aformat=s16", "-c:a", "pcm_s24le", wav)
        run("flac", "-s", "-f", "-8", wav, "-o", self.p("w.flac"))
        ref = ff_decode(self.p("w.flac"), self.p("ref.f64"))
        self.decode_vs("flac", "24b container, 16b content (wasted bits)", self.p("w.flac"), ref, True,
                       ref_scale=1 << 23)
        # ffmpeg's encoder: different choices of order, partition and stereo mode.
        for lvl in ("0", "8", "12"):
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(44100, 2, 0.2),
                "-c:a", "flac", "-compression_level", lvl, "-sample_fmt", "s16", self.p("ff.flac"))
            ref = ff_decode(self.p("ff.flac"), self.p("ref.f64"))
            self.decode_vs("flac", f"ffmpeg encoder level {lvl}", self.p("ff.flac"), ref, True)

    def alac(self):
        for fmt, bits in (("s16p", 16), ("s32p", 24)):
            for ch in (1, 2, 3, 4, 5, 6, 7, 8):
                for rate in (44100, 96000):
                    src = self.p("a.m4a")
                    p = run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.12),
                            "-c:a", "alac", "-sample_fmt", fmt, src, check=False)
                    if p.returncode != 0:
                        continue
                    ref = ff_decode(src, self.p("ref.f64"))
                    self.decode_vs("alac", f"{rate} {ch}ch {bits}b", src, ref, True,
                                   ref_scale=float(1 << (bits - 1)))

    def mp3(self):
        for rate in (8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000):
            for ch, mode in ((1, None), (2, "0"), (2, "1")):
                for br in ("vbr", "32k", "128k"):
                    src = self.p("m.mp3")
                    args = ["-c:a", "libmp3lame"]
                    args += ["-q:a", "4"] if br == "vbr" else ["-b:a", br]
                    if mode is not None:
                        args += ["-joint_stereo", mode]
                    p = run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.6),
                            "-ac", str(ch), *args, src, check=False)
                    if p.returncode != 0:
                        continue
                    # Gapless (2.10.2): the LAME tag's delay and padding trimmed, as
                    # ffmpeg plays it, to the sample.
                    ref = ff_decode(src, self.p("ref.f64"))
                    js = {None: "mono", "0": "stereo", "1": "joint"}[mode]
                    self.decode_vs("mp3", f"{rate} {js} {br}", src, ref, False)
        # Layer II from twolame (incl. joint stereo with intensity), against mpg123.
        for rate in (32000, 44100, 48000, 16000, 24000):
            for mode in ("m", "s", "j"):
                wav = self.p("in.wav")
                ch = 1 if mode == "m" else 2
                run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.5),
                    "-c:a", "pcm_s16le", wav)
                mp2 = self.p("t.mp2")
                p = run("twolame", "--quiet", "-m", mode, "-b", "96" if ch == 1 else "192", wav, mp2,
                        check=False)
                if p.returncode != 0:
                    continue
                raw = run("mpg123", "-q", "-s", mp2).stdout
                ref16 = array.array("h")
                ref16.frombytes(raw[: len(raw) // 2 * 2])
                ref = array.array("d", (x / 32768.0 for x in ref16))
                self.decode_vs("mp2", f"twolame {rate} -m {mode}", mp2, ref, False)

    def aac(self):
        for rate in (8000, 11025, 16000, 22050, 24000, 32000, 44100, 48000, 64000, 88200, 96000):
            for ch in (1, 2, 3, 6):
                for container in ("adts", "m4a"):
                    src = self.p("a." + ("aac" if container == "adts" else "m4a"))
                    args = ["-c:a", "aac", "-aac_pns", "0", "-b:a", f"{48 * ch}k"]
                    if container == "adts":
                        args += ["-f", "adts"]
                    p = run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.5),
                            *args, src, check=False)
                    if p.returncode != 0:
                        continue
                    name = f"{rate} {ch}ch {container}"
                    # m4a: the edit list's priming and end trim applied (2.10.2), as
                    # ISO 14496-12 cuts it (see mp4_iso_ref).
                    if container == "m4a":
                        ref = mp4_iso_ref(src, self.p("ref.f64"), 0, ch)
                    else:
                        ref = ff_decode(src, self.p("ref.f64"))
                    self.decode_vs("aac", name, src, ref, False)
        # Every layout ffmpeg's encoder writes (decoded since 2.10.3), in ffmpeg's
        # channel order: channel configurations 1 to 6 (and 12 in MP4), the rest as a
        # program_config_element, in the stream for ADTS and in the
        # AudioSpecificConfig for MP4. ffmpeg cannot write 7.1 as ADTS.
        for layout, ch in (("2.1", 3), ("3.0", 3), ("quad", 4), ("4.0", 4), ("5.0", 5),
                           ("5.1", 6), ("5.1(side)", 6), ("6.0", 6), ("hexagonal", 6),
                           ("6.1", 7), ("7.1", 8), ("7.1(wide)", 8)):
            for container in ("adts", "m4a"):
                if layout == "7.1" and container == "adts":
                    continue
                src = self.p("a." + ("aac" if container == "adts" else "m4a"))
                args = ["-af", f"aformat=channel_layouts={layout}", "-c:a", "aac", "-aac_pns", "0",
                        "-b:a", f"{48 * ch}k"] + (["-f", "adts"] if container == "adts" else [])
                p = run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(48000, ch, 0.5),
                        *args, src, check=False)
                name = f"48000 {layout} {container}"
                if p.returncode != 0:
                    self.record("aac", name, False, "ffmpeg cannot write it")
                    continue
                if container == "m4a":
                    ref = mp4_iso_ref(src, self.p("ref.f64"), 0, ch)
                else:
                    ref = ff_decode(src, self.p("ref.f64"))
                self.decode_vs("aac", name, src, ref, False)

    def gapless(self):
        # MP4 track selection: ffmpeg's default audio stream (enabled tracks first,
        # then the most channels, then the first) and every track by index. AAC and
        # ALAC, each to the sample and to ffmpeg's length.
        srcs = [("sine=f=440:d=0.3:sample_rate=44100", 1),
                ("aevalsrc=0.3*sin(2*PI*660*t)|0.2*sin(2*PI*990*t):s=44100:d=0.25", 2),
                ("sine=f=550:d=0.2:sample_rate=44100", 1)]
        layouts = [(1, 1, 1), (0, 1, 1), (1, 0, 1), (0, 0, 1), (0, 0, 0), (1, 1, 0)]
        for codec in ("aac", "alac"):
            for lay in layouts:
                src = self.p("t.m4a")
                args = []
                for f, _ in srcs:
                    args += ["-f", "lavfi", "-i", f]
                for i in range(len(srcs)):
                    args += ["-map", f"{i}:a"]
                for i, en in enumerate(lay):
                    args += [f"-disposition:a:{i}", "default" if en else "0"]
                cargs = ["-c:a", "aac", "-aac_pns", "0", "-b:a", "64k"] if codec == "aac" else ["-c:a", "alac"]
                run("ffmpeg", "-v", "error", "-y", *args, *cargs, src)
                name = f"{codec} tracks enabled={''.join(map(str, lay))}"
                p = run(CLI, "tracks", src, check=False)
                fields = p.stdout.decode().split()
                if p.returncode != 0 or len(fields) < 2:
                    self.record("gapless", name, False, "shravan error")
                    continue
                chans = [c for _, c in srcs]
                want = ffmpeg_default_track(src, chans)
                ref = mp4_iso_ref(src, self.p("ref.f64"), want, chans[want])
                got, info = shravan_decode(src, self.p("got.f64"))
                lossless = codec == "alac"
                e = maxerr(ref, got) if got is not None else 1e9
                ok = (fields[0] == "3" and fields[1] == str(want) and got is not None and len(got) == len(ref)
                      and (e == 0 if lossless else e <= 2.0))
                self.record("gapless", name + " default", ok,
                            f"default={fields[1]} n={len(got) if got else 0}/{len(ref)} maxerr={e:.3f}")
                for i in range(len(srcs)):
                    ref = mp4_iso_ref(src, self.p("ref.f64"), i, chans[i])
                    q = run(CLI, "track", src, str(i), self.p("got.f64"), check=False)
                    got = f64(self.p("got.f64")) if q.returncode == 0 else None
                    e = maxerr(ref, got) if got is not None else 1e9
                    ok = got is not None and len(got) == len(ref) and (e == 0 if lossless else e <= 2.0)
                    self.record("gapless", f"{name} track {i}", ok, f"n={len(got) if got else 0}/{len(ref)} maxerr={e:.3f}")

    def opus(self):
        # libopus built with deep PLC (DRED or OSCE) skips the SILK PLC glue fade at
        # 16 kHz, which a default build applies and shravan follows: wideband SILK and
        # hybrid concealment is checked only against a default build. `opusref caps`
        # tells them apart by decoder size (the DRED entry points are exported as
        # stubs either way). OPUS_PREFIX in scripts/interop.sh links a default build.
        caps = run(OPUSREF, "caps").stdout.decode()
        deep = "deep_plc 1" in caps
        print("  " + caps.strip())
        cases = [(c, ch, ()) for c in range(32) for ch in (1, 2)]
        cases += [(9, 2, ("dur=2880", "fec=20")), (13, 1, ("fec=20",)), (9, 2, ("dur=2880", "sig=1")),
                  (31, 2, ("sw=3", "chsw=4")), (1, 2, ("bwsw=5",)), (0, 2, ("fsw=4",)),
                  (31, 2, ("bwsw=4",)), (31, 2, ("sw=2", "br=10000", "dur=1920")),
                  (5, 2, ("dtx=1", "sig=3", "voip=1")), (31, 1, ("dtx=1", "sig=3")),
                  (28, 2, ("lose=5",)), (5, 2, ("lose=5",)), (31, 2, ("swmb=1", "lose=2")),
                  (1, 2, ("auto=1", "br=20000", "dur=1920")), (1, 1, ("auto=1", "voip=1", "br=12000"))]
        if not deep:
            # A default libopus build: wideband SILK and hybrid concealment, every frame
            # size and both channel counts, under loss and DTX.
            for cfg in (8, 9, 10, 11, 12, 13, 14, 15):
                for ch in (1, 2):
                    cases += [(cfg, ch, ("lose=3",)), (cfg, ch, ("dtx=1", "sig=3", "voip=1"))]
            cases += [(9, 2, ("sw=3", "lose=4")), (13, 2, ("fec=20", "lose=4"))]
        else:
            print("  deep-PLC libopus: wideband SILK/hybrid loss and DTX not checked"
                  " (OPUS_PREFIX=<default build> ./scripts/interop.sh to include them)")
        for cfg, ch, opts in cases:
            pk, ref = self.p("t.pk"), self.p("t.ref")
            out = run(OPUSREF, "gen", str(cfg), str(ch), "40", pk, ref, *opts).stdout.decode().strip()
            p = run(CLI, "opusdec", pk, str(ch), self.p("got.f64"), check=False)
            if p.returncode != 0:
                self.record("opus", f"cfg {cfg} {ch}ch {' '.join(opts)}", False, "shravan error")
                continue
            r, g = f64(ref), f64(self.p("got.f64"))
            e = maxerr(r, g)
            # Concealed CELT frames extrapolate through a 24-tap LPC filter; float32 in
            # libopus against f64 here differs by a few LSB on loud frames.
            tol = 4.0 if any(o.startswith("lose") for o in opts) else 2.0
            self.record("opus", f"cfg {cfg} {ch}ch {' '.join(opts)}",
                        len(r) == len(g) and e <= tol, f"maxerr={e:.2f} ({out.split('|')[0].strip()})")
        # Ogg Opus end to end from ffmpeg, with its pre-skip and end trimming.
        for ch, br, app, fd in ((1, "64k", "audio", "20"), (2, "96k", "audio", "10"),
                                (2, "128k", "lowdelay", "2.5"), (1, "16k", "voip", "20"),
                                (2, "24k", "voip", "40"), (2, "32k", "voip", "60")):
            src = self.p("o.opus")
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(48000, ch, 1.0),
                "-c:a", "libopus", "-b:a", br, "-application", app, "-frame_duration", fd, src)
            ref = ff_decode(src, self.p("ref.f64"), "-c:a", "libopus")
            self.decode_vs("ogg", f"opus {ch}ch {br} {app} {fd}ms", src, ref, False)
        # Multistream Ogg Opus (2.10.3): RFC 7845 family 1 in ffmpeg's channel order
        # (its libopus wrapper reorders as shravan does), families 255 and 2 in
        # mapping order (the wrapper's decode put back, libopus_mapping_order).
        # Frame sizes over 20 ms give self-delimited packets with framing codes 1
        # to 3.
        for layout, ch, fam, br, fd, vbr in (("3.0", 3, 1, "160k", "20", "on"),
                                             ("quad", 4, 1, "192k", "20", "on"),
                                             ("5.0", 5, 1, "240k", "20", "on"),
                                             ("5.1", 6, 1, "256k", "20", "on"),
                                             ("6.1", 7, 1, "320k", "20", "on"),
                                             ("7.1", 8, 1, "384k", "20", "on"),
                                             ("5.1", 6, 1, "256k", "40", "off"),
                                             ("5.1", 6, 1, "128k", "60", "on"),
                                             ("7.1", 8, 1, "256k", "120", "on"),
                                             ("quad", 4, 255, "192k", "20", "on"),
                                             ("5.1", 6, 255, "256k", "40", "on"),
                                             ("3.0", 3, 255, "126k", "20", "on"),
                                             ("quad", 4, 2, "192k", "20", "on"),
                                             ("5.1", 6, 2, "256k", "20", "on")):
            src = self.p("m.opus")
            name = f"opus {layout} family {fam} {fd}ms vbr={vbr}"
            p = run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(48000, ch, 1.0),
                    "-af", f"aformat=channel_layouts={layout}", "-c:a", "libopus", "-b:a", br,
                    "-frame_duration", fd, "-vbr", vbr, "-mapping_family", str(fam), src, check=False)
            if p.returncode != 0:
                self.record("ogg", name, False, "ffmpeg cannot write it")
                continue
            ref = ff_decode(src, self.p("ref.f64"), "-c:a", "libopus")
            if fam != 1:
                ref = libopus_mapping_order(ref, ch)
            self.decode_vs("ogg", name, src, ref, False)

    # -- encode direction -------------------------------------------------------

    def flac_encoder(self):
        """shravan's FLAC levels against libFLAC: flac -t passes, ffmpeg decodes every
        sample back, and the audio frames are no larger than `flac -L`'s on the same
        input (1% allowed; metadata excluded, as flac pads and shravan writes a seek
        table). Then depths and channel counts the integer-grid sweep skips."""
        def frames(path):
            b = open(path, "rb").read()
            pos = 4
            while True:
                last = b[pos] & 0x80
                pos += 4 + int.from_bytes(b[pos + 1:pos + 4], "big")
                if last:
                    return len(b) - pos

        def judge(name, out, grid, bits):
            t = run("flac", "-t", "-s", out, check=False)
            back = ff_decode(out, self.p("back.f64"))
            e = maxerr(grid, back, float(1 << (bits - 1)))
            ok = t.returncode == 0 and len(back) == len(grid) and e == 0
            return ok, f"maxerr={e:.3f} LSB n={len(back)}/{len(grid)} flac -t {'ok' if t.returncode == 0 else 'FAILED'}"

        wav = self.p("lv.wav")
        run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(44100, 2, 3.0),
            "-c:a", "pcm_s16le", wav)
        grid = ff_decode(wav, self.p("lv.f64"))
        for level in range(9):
            out = self.p("lv.flac")
            p = run(CLI, "encode", self.p("lv.f64"), "flac", "44100", "2", "16", out, str(level),
                    check=False)
            name = f"flac level {level} vs flac -{level}"
            if p.returncode != 0:
                self.record("encode", name, False, "shravan error " + p.stdout.decode().strip())
                continue
            ok, detail = judge(name, out, grid, 16)
            ref = self.p("ref.flac")
            run("flac", "-s", "-f", f"-{level}", "--no-padding", "--no-seektable", wav, "-o", ref)
            ours, theirs = frames(out), frames(ref)
            ok = ok and ours <= theirs * 1.01
            self.record("encode", name, ok, f"{detail} frames {ours}/{theirs} ({100.0 * (ours - theirs) / theirs:+.2f}%)")
        for bits, ch, rate in ((12, 2, 48000), (20, 2, 96000), (32, 2, 48000), (16, 8, 48000),
                               (24, 3, 44100)):
            codec = {12: "pcm_s16le", 16: "pcm_s16le", 20: "pcm_s24le", 24: "pcm_s24le",
                     32: "pcm_s32le"}[bits]
            src = self.p("dp.wav")
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.5),
                "-c:a", codec, src)
            g = ff_decode(src, self.p("dp.f64"))
            # Onto a grid of `bits` bits, so the encoder has an exact target.
            step = float(1 << (bits - 1))
            g = array.array("d", (round(v * step) / step for v in g))
            with open(self.p("dp.f64"), "wb") as fh:
                g.tofile(fh)
            for level in (0, 5, 8):
                out = self.p("dp.flac")
                p = run(CLI, "encode", self.p("dp.f64"), "flac", str(rate), str(ch), str(bits),
                        out, str(level), check=False)
                name = f"flac {bits}b {ch}ch {rate} level {level}"
                if p.returncode != 0:
                    self.record("encode", name, False, "shravan error " + p.stdout.decode().strip())
                    continue
                ok, detail = judge(name, out, g, bits)
                self.record("encode", name, ok, detail)
        # A seek point every 10 seconds, read back by metaflac.
        run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(8000, 1, 25.0),
            "-c:a", "pcm_s16le", self.p("long.wav"))
        ff_decode(self.p("long.wav"), self.p("long.f64"))
        run(CLI, "encode", self.p("long.f64"), "flac", "8000", "1", "16", self.p("long.flac"), "5")
        listing = run("metaflac", "--list", "--block-type=SEEKTABLE", self.p("long.flac")).stdout.decode()
        want = ["sample_number=0,", "sample_number=77824,", "sample_number=159744,"]
        ok = all(w in listing for w in want) and "seek points: 3" in listing
        self.record("encode", "flac seek table (metaflac)", ok, " ".join(l.strip() for l in listing.splitlines() if "point" in l))

    def encoders(self):
        for rate in (8000, 44100, 96000):
            for ch in (1, 2, 6):
                for kind, bits, ext in (("wav", 8, "wav"), ("wav", 16, "wav"), ("wav", 24, "wav"),
                                        ("wav", 32, "wav"), ("aiff", 16, "aiff"), ("aiff", 24, "aiff"),
                                        ("flac", 16, "flac"), ("flac", 24, "flac"), ("flac", 8, "flac")):
                    # Integer-grid input: a lossless encoder must hand every value back.
                    wav = self.p("grid.wav")
                    codec = {8: "pcm_u8", 16: "pcm_s16le", 24: "pcm_s24le", 32: "pcm_s32le"}[bits]
                    run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(rate, ch, 0.1),
                        "-c:a", codec, wav)
                    grid = ff_decode(wav, self.p("grid.f64"))
                    out = self.p("enc." + ext)
                    p = run(CLI, "encode", self.p("grid.f64"), kind, str(rate), str(ch), str(bits), out,
                            check=False)
                    name = f"{kind} {bits}b {rate} {ch}ch"
                    if p.returncode != 0:
                        self.record("encode", name, False, "shravan error " + p.stdout.decode().strip())
                        continue
                    back = ff_decode(out, self.p("back.f64"))
                    e = maxerr(grid, back, float(1 << (bits - 1)))
                    ok = len(back) == len(grid) and e == 0
                    detail = f"maxerr={e:.3f} LSB n={len(back)}/{len(grid)}"
                    if kind == "flac":
                        t = run("flac", "-t", "-s", out, check=False)
                        ok = ok and t.returncode == 0
                        detail += " flac -t " + ("ok" if t.returncode == 0 else "FAILED")
                    self.record("encode", name, ok, detail)
        self.flac_encoder()
        for kind in ("wavf32", "wavf64"):
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(48000, 2, 0.1),
                "-f", "f64le", "-c:a", "pcm_f64le", self.p("x.f64"))
            x = f64(self.p("x.f64"))
            run(CLI, "encode", self.p("x.f64"), kind, "48000", "2", "0", self.p("enc.wav"))
            back = ff_decode(self.p("enc.wav"), self.p("back.f64"))
            want = x
            if kind == "wavf32":
                # The f32 samples must be ffmpeg's own rounding of the f64 input
                # (nearest, ties to even); before 2.9.3 shravan truncated.
                run("ffmpeg", "-v", "error", "-y", "-f", "f64le", "-ar", "48000", "-ac", "2",
                    "-i", self.p("x.f64"), "-c:a", "pcm_f32le", self.p("ref32.wav"))
                want = ff_decode(self.p("ref32.wav"), self.p("ref32.f64"))
            e = maxerr(want, back, 1.0)
            self.record("encode", kind, len(back) == len(want) and e == 0, f"maxerr={e:.3g}")
        # AAC: what ffmpeg and faad make of shravan's stream, at every band
        # layout and from low to high rates (the chirp, 300 + 2 sweep t Hz, held
        # under 6.3 kHz, inside both encoders' bandwidth at 32 kbit/s a channel, so
        # the comparison is of coding, not of where each draws the cutoff; at least
        # 12000 samples, past the correlation's warm-up). The stream must decode to the input at
        # exactly the encoder's 1024 samples of priming, within 0.02 of the
        # correlation ffmpeg's own encoder reaches on the same input at the same
        # rate; spend at most the rate asked (2% and 1 kbit/s for the reservoir on
        # a short clip); and keep every frame within AAC-LC's 6144 bits a channel.
        faad = shutil.which("faad")
        for rate in (8000, 11025, 16000, 22050, 24000, 32000, 44100, 48000, 64000, 96000):
            for ch in (1, 2):
                dur = max(0.5, 12000 / rate)
                run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i",
                    signal(rate, ch, dur, 0.3, min(rate // 5, int(3000 / dur))),
                    "-f", "f64le", "-c:a", "pcm_f64le", self.p("x.f64"))
                x = f64(self.p("x.f64"))
                run("ffmpeg", "-v", "error", "-y", "-f", "f64le", "-ar", str(rate), "-ac", str(ch),
                    "-i", self.p("x.f64"), "-c:a", "pcm_s16le", self.p("x.wav"))
                for br_ch in (32000, 64000, 128000):
                    if br_ch > rate * 6:
                        continue
                    br = br_ch * ch
                    name = f"aac {rate} {ch}ch {br // 1000}k"
                    p = run(CLI, "encode", self.p("x.f64"), "aac", str(rate), str(ch), str(br),
                            self.p("enc.aac"), check=False)
                    if p.returncode != 0:
                        self.record("encode", name, False, "shravan error")
                        continue
                    data = open(self.p("enc.aac"), "rb").read()
                    sizes, pos = [], 0
                    while pos + 7 <= len(data) and data[pos] == 0xFF and data[pos + 1] & 0xF0 == 0xF0:
                        sizes.append(((data[pos + 3] & 3) << 11) | (data[pos + 4] << 3) | (data[pos + 5] >> 5))
                        pos += sizes[-1]
                    # over the coded frames (priming and flush), less the one
                    # reservoir a constant-rate stream may start with
                    coded = len(sizes) * 1024 / rate
                    kbps = (len(data) * 8 - 6144 * ch) / coded / 1000
                    frames_ok = pos == len(data) and max(sizes) - 7 <= 768 * ch
                    back = ff_decode(self.p("enc.aac"), self.p("back.f64"))
                    c, lag = best_lag_corr(x, back, ch, 1034, 2048, 1014)
                    fe = run("ffmpeg", "-v", "error", "-y", "-i", self.p("x.wav"), "-c:a", "aac",
                             "-b:a", str(br), "-f", "adts", self.p("ff.aac"), check=False)
                    cf, ffk = 0.0, 0.0
                    if fe.returncode == 0:
                        cf, _ = best_lag_corr(x, ff_decode(self.p("ff.aac"), self.p("ffb.f64")), ch, 1034, 2048, 1014)
                        ffk = (os.path.getsize(self.p("ff.aac")) * 8 - 6144 * ch) / coded / 1000
                    # ffmpeg's bar holds where it spends no more than 10% over
                    # shravan (it overshoots most at the lowest rates per sample).
                    bar = c >= cf - 0.02 or ffk > 1.1 * kbps
                    ok = lag == 1024 and c > 0.9 and bar and frames_ok and kbps <= br / 1000
                    detail = (f"corr={c:.4f} (ffmpeg's encoder {cf:.4f} at {ffk:.1f}) lag={lag} {kbps:.1f} kbit/s "
                              f"largest frame {(max(sizes) - 7) * 8} bits")
                    if faad:
                        fa = run(faad, "-q", "-o", self.p("faad.wav"), self.p("enc.aac"), check=False)
                        fok = fa.returncode == 0
                        if fok:
                            with wave.open(self.p("faad.wav")) as w:
                                frate, fn = w.getframerate(), w.getnframes()
                            if frate == rate:
                                cfa, _ = best_lag_corr(x, ff_decode(self.p("faad.wav"), self.p("faad.f64")), ch, 1034, 2048, 1014)
                                fok = cfa > c - 0.002
                                detail += f" faad corr={cfa:.4f}"
                            else:
                                # Below 32 kHz faad assumes implicit SBR and plays
                                # the stream at twice the rate: every frame, doubled.
                                fok = frate == 2 * rate and fn == 2048 * len(sizes)
                                detail += f" faad at {frate} Hz, {fn} frames"
                        else:
                            detail += " faad FAILED"
                        ok = ok and fok
                    self.record("encode", name, ok, detail)
        # CELT: what libopus makes of shravan's frames.
        for ch in (1, 2):
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", signal(48000, ch, 0.5, 0.3),
                "-f", "f64le", "-c:a", "pcm_f64le", self.p("x.f64"))
            x = f64(self.p("x.f64"))
            run(CLI, "celtenc", self.p("x.f64"), str(ch), str(160 * ch), self.p("c.pk"))
            run(OPUSREF, "dec", self.p("c.pk"), str(ch), self.p("back.f64"))
            back = f64(self.p("back.f64"))
            c, lag = best_lag_corr(x, back, ch, 200)
            self.record("encode", f"celt {ch}ch -> libopus", c > 0.9 and lag == 0, f"corr={c:.4f} lag={lag}")


    def silk(self):
        """shravan's SILK encoder (silk_encode_opus, complexity 1) beside libopus's
        own (opus_encode at the internal rate, forced to SILK at that bandwidth,
        complexity 1), libopus decoding both: every rate and packet duration, VBR
        and CBR, on pink noise, on the sweep's tones and on a voiced source (eight
        harmonics of a 110 Hz pitch with vibrato, falling 6 dB an octave: long-term
        prediction on nearly every frame). Each must follow the input at libopus's
        lag, correlate at least 0.8 and within 0.03 of libopus's, and spend
        libopus's bits: within 10% under VBR, the same packet size under CBR. On
        the voiced source the correlation is flat to the fourth decimal across
        neighbouring lags, so its best lag may land a sample from libopus's; the
        noise and the tones pin the delay to the sample.
        libopus's Opus layer high-passes its input at 3 Hz (dc_reject), which costs
        it correlation on pink noise; shravan's payloads are silk_Encode's byte for
        byte on the suite's inputs, voiced ones included (src/silk_tests.cyr)."""
        def sizes(path):
            d = open(path, "rb").read()
            out, p = [], 0
            while p + 4 <= len(d):
                n = struct.unpack_from("<I", d, p)[0]
                out.append(n)
                p += 4 + n
            return out
        ph = "(110*t+15*sin(2*PI*0.7*t)/(2*PI*0.7))"
        voiced = ("aevalsrc=" + "+".join(f"{0.3 / k:.4f}*sin(2*PI*{k}*{ph})" for k in range(1, 9))
                  + "+0.01*random(0)")
        for src in ("pink", "tones", "voiced"):
            for fs in (8000, 12000, 16000):
                br = {8000: 12000, 12000: 16000, 16000: 20000}[fs]
                for ms in (10, 20, 40, 60):
                    for cbr in (0, 1):
                        lav = {"pink": f"anoisesrc=color=pink:r={fs}:a=0.25:d=1.5:seed=7",
                               "tones": signal(fs, 1, 1.5),
                               "voiced": voiced + f":s={fs}:d=1.5"}[src]
                        run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", lav, "-ac", "1",
                            "-f", "f64le", "-c:a", "pcm_f64le", self.p("s.f64"))
                        x = f64(self.p("s.f64"))
                        res = {}
                        for who in ("shravan", "libopus"):
                            pk, back = self.p(who + ".pk"), self.p(who + ".dec")
                            if who == "shravan":
                                run(CLI, "silkenc", self.p("s.f64"), str(fs), str(br), "1", str(ms), str(cbr), pk)
                            else:
                                run(OPUSREF, "silk", self.p("s.f64"), str(fs), str(br), str(ms), str(cbr), pk)
                            run(OPUSREF, "dec", pk, "1", back, str(fs))
                            lag0 = 13 * fs // 2000                     # 6.5 ms
                            c, lag = best_lag_corr(x, f64(back), 1, lag0 + 3, fs // 10, lag0 - 3)
                            sz = sizes(pk)
                            res[who] = (c, lag, sum(sz) * 8 / (len(sz) * ms / 1000), set(sz))
                        (c, lag, rate, sz), (lc, llag, lrate, lsz) = res["shravan"], res["libopus"]
                        ok = abs(lag - llag) <= (1 if src == "voiced" else 0) and c >= 0.8 and c >= lc - 0.03
                        ok = ok and (sz == lsz and len(sz) == 1 if cbr else abs(rate / lrate - 1) <= 0.10)
                        self.record("encode", f"silk {src} {fs // 1000}k {ms}ms {'cbr' if cbr else 'vbr'} -> libopus", ok,
                                    f"corr={c:.4f} (libopus {lc:.4f}) lag={lag} ({llag}) {rate / 1000:.1f} kb/s ({lrate / 1000:.1f})")
        self.silk_api_fec_dtx(voiced, sizes)
        self.silk_stereo(ph, sizes)

    def celt(self):
        """shravan's CELT encoder (src/celt_enc.cyr, libopus's celt_encode_with_ec)
        beside libopus's own (opus_encode forced to CELT only, fullband), libopus
        decoding both: mono and stereo, 2.5 to 20 ms, complexity 0, 5 and 10, VBR
        and CBR, on pink noise, the sweep's tones and the voiced source. Each must
        correlate with its input within 0.01 of libopus's and spend libopus's bits
        within 10%. Its frames are libopus's portable C build's byte for byte
        (src/celt_enc_tests.cyr); a distribution's libopus takes its x86 SSE
        paths, which round some float sums in another order, and opus_encode adds
        its 3 Hz high-pass and delay, so here the measure is the result."""
        def frames(path):
            d = open(path, "rb").read()
            out, q = [], 0
            while q + 4 <= len(d):
                n = struct.unpack_from("<I", d, q)[0]
                out.append(n)
                q += 4 + n
            return out
        ph = "(110*t+15*sin(2*PI*0.7*t)/(2*PI*0.7))"
        voiced = ("aevalsrc=" + "+".join(f"{0.3 / k:.4f}*sin(2*PI*{k}*{ph})" for k in range(1, 9))
                  + "+0.01*random(0)")
        for src in ("pink", "tones", "voiced"):
            for ch in (1, 2):
                lav = {"pink": "anoisesrc=color=pink:r=48000:a=0.25:d=1.2:seed=11",
                       "tones": signal(48000, 1, 1.2),
                       "voiced": voiced + ":s=48000:d=1.2"}[src]
                run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", lav, "-ac", str(ch),
                    "-f", "f64le", "-c:a", "pcm_f64le", self.p("c.f64"))
                if ch == 2:
                    # an image that is not mono: the right channel delayed 1.5 ms, 4 dB down
                    x = f64(self.p("c.f64"))
                    st = []
                    for i in range(0, len(x), 2):
                        j = i - 144
                        st += [x[i], 0.63 * x[j] if j >= 0 else 0.0]
                    open(self.p("c.f64"), "wb").write(struct.pack("<%dd" % len(st), *st))
                x = f64(self.p("c.f64"))
                for n in (960, 480, 240, 120):
                    for cx in (0, 5, 10):
                        for cbr in (0, 1):
                            br = 64000 * ch if n >= 480 else 96000 * ch
                            res = {}
                            for who in ("shravan", "libopus"):
                                pk, back = self.p(who + ".pk"), self.p(who + ".dec")
                                if who == "shravan":
                                    keys = [f"n={n}", f"cx={cx}", f"bitrate={br}", "toc=1"] + ([] if cbr else ["vbr=1"])
                                    run(CLI, "celtlib", self.p("c.f64"), pk, "48000", str(ch), *keys)
                                else:
                                    run(OPUSREF, "celt", self.p("c.f64"), str(ch), str(br), str(n), str(cx), str(cbr), pk)
                                run(OPUSREF, "dec", pk, str(ch), back)
                                # CELT's delay is its 120-sample overlap; opus_encode adds 4 ms
                                lag0 = 120 if who == "shravan" else 312
                                c, lag = best_lag_corr(x, f64(back), ch, lag0 + 3, 2048, lag0 - 3)
                                sz = frames(pk)
                                res[who] = (c, lag, sum(s - 1 for s in sz) * 8 / (len(sz) * n / 48000))
                            (c, lag, rate), (lc, llag, lrate) = res["shravan"], res["libopus"]
                            ok = c >= lc - 0.01 and lag == 120 and llag == 312 and abs(rate / lrate - 1) <= 0.10
                            self.record("encode", f"celt {src} {ch}ch {n} cx{cx} {'cbr' if cbr else 'vbr'} -> libopus", ok,
                                        f"corr={c:.4f} (libopus {lc:.4f}) lag={lag} ({llag}) "
                                        f"{rate / 1000:.1f} kb/s ({lrate / 1000:.1f})")

    def opusenc(self):
        """The Opus encoder layer (opus_layer), on the silk group's sources."""
        def sizes(path):
            d = open(path, "rb").read()
            out, p = [], 0
            while p + 4 <= len(d):
                n = struct.unpack_from("<I", d, p)[0]
                out.append(n)
                p += 4 + n
            return out
        ph = "(110*t+15*sin(2*PI*0.7*t)/(2*PI*0.7))"
        voiced = ("aevalsrc=" + "+".join(f"{0.3 / k:.4f}*sin(2*PI*{k}*{ph})" for k in range(1, 9))
                  + "+0.01*random(0)")
        self.opus_layer(ph, sizes, voiced)

    def opus_layer(self, ph, sizes, voiced):
        """2.12.3. shravan's opus_encode (OPUS_APPLICATION_RESTRICTED_SILK) beside
        libopus's (build/opusenc-ref: the same application, or with a libopus older
        than 1.6 OPUS_APPLICATION_AUDIO forced to SILK at wideband at most), libopus
        decoding both, at the input's rate: pink noise, the sweep's tones and the
        voiced source, at 8 to 48 kHz, 10 to 120 ms, VBR and CBR, complexity 1 to
        10 (the delayed-decision quantiser from 2), mono and stereo. Each must
        follow the input at libopus's lag (the lookahead, 6.5 ms; within a sample
        on the voiced source), correlate at least 0.8 and within 0.03 of libopus's,
        at libopus's bits (within 10% under VBR, the same sizes under CBR). Then
        in-band FEC by loss with every fifth packet lost, recovering better than
        concealment and within 0.03 of libopus's recovery; DTX on digital silence
        between bursts (this layer's DTX), its packets left out where libopus's are
        (within two; against the AUDIO fallback, whose DTX follows its tonality
        analysis, only shravan's); and a bit rate falling to 8 kbit/s and back, the bandwidth
        going down through SILK's transition and up again where libopus's does
        (within ten packets). (shravan's packets are libopus's byte for byte on the
        suite's inputs, src/opus_enc_tests.cyr.)"""
        if not os.path.exists(OPUSENCREF):
            self.record("encode", "opus encode (RESTRICTED_SILK)", False, "no build/opusenc-ref (scripts/interop.sh)")
            return
        def voice(f0, seed):
            p = ph.replace("110*t", f"{f0}*t")
            return "+".join(f"{0.3 / k:.4f}*sin(2*PI*{k}*{p})" for k in range(1, 9)) + f"+0.01*random({seed})"

        def make(src, api, ch, d):
            if ch == 2:
                lav = f"aevalsrc=({voice(110, 0)})|0.5*({voice(110, 0)})+0.5*({voice(165, 1)}):s={api}:d={d}"
            else:
                lav = {"pink": f"anoisesrc=color=pink:r={api}:a=0.25:d={d}:seed=7",
                       "tones": signal(api, 1, d),
                       "voiced": voiced + f":s={api}:d={d}"}[src]
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", lav, "-ac", str(ch),
                "-f", "f64le", "-c:a", "pcm_f64le", self.p("s.f64"))
            x = f64(self.p("s.f64"))
            return [x[c::ch] for c in range(ch)]

        def encode(who, api, ch, keys):
            pk = self.p(who + ".pk")
            if who == "shravan":
                run(CLI, "opusenc", self.p("s.f64"), pk, str(api), str(ch), *keys)
            else:
                out = run(OPUSENCREF, self.p("s.f64"), pk, str(api), str(ch), *keys).stdout
                mode[0] = (out.decode() if isinstance(out, bytes) else out).split("\n")[0]
            return pk

        def follow(xs, path, api, ch):
            y = f64(path)
            lag0 = 13 * api // 2000
            return [best_lag_corr(x, y[c::ch], 1, lag0 + 3, api // 10, lag0 - 3) for c, x in enumerate(xs)]

        mode = [""]
        cases = [(src, api, 1, ms, cbr, br, cx) for src in ("pink", "tones", "voiced")
                 for api, ms, cbr, br, cx in ((48000, 20, 0, 24000, 9), (48000, 60, 1, 20000, 1), (48000, 10, 0, 24000, 5),
                                              (48000, 120, 0, 24000, 9), (24000, 40, 1, 18000, 3), (16000, 20, 0, 20000, 10),
                                              (12000, 80, 0, 16000, 7), (8000, 20, 1, 12000, 9))]
        cases += [("stereo", 48000, 2, 20, 0, 40000, 9), ("stereo", 48000, 2, 80, 1, 32000, 1),
                  ("stereo", 24000, 2, 20, 0, 36000, 6), ("stereo", 16000, 2, 60, 0, 30000, 10)]
        for src, api, ch, ms, cbr, br, cx in cases:
            xs = make(src, api, ch, 1.5)
            keys = [f"bitrate={br}", f"ms={ms}", f"vbr={1 - cbr}", f"cx={cx}"]
            res = {}
            for who in ("shravan", "libopus"):
                pk = encode(who, api, ch, keys)
                run(OPUSREF, "dec", pk, str(ch), self.p(who + ".dec"), str(api))
                sz = sizes(pk)
                res[who] = (follow(xs, self.p(who + ".dec"), api, ch), sum(sz) * 8 / (len(sz) * ms / 1000), set(sz))
            (cs, rate, sz), (lcs, lrate, lsz) = res["shravan"], res["libopus"]
            tol = 1 if src in ("voiced", "stereo") else 0
            ok = all(abs(c[1] - l[1]) <= tol and c[0] >= 0.8 and c[0] >= l[0] - 0.03 for c, l in zip(cs, lcs))
            ok = ok and (sz == lsz if cbr else abs(rate / lrate - 1) <= 0.10)
            self.record("encode", f"opus {src} {api // 1000}k {ms}ms {'cbr' if cbr else 'vbr'} cx{cx} -> libopus", ok,
                        "corr %s (libopus %s) lag=%s (%s) %.1f kb/s (%.1f)"
                        % ("/".join("%.4f" % c[0] for c in cs), "/".join("%.4f" % c[0] for c in lcs),
                           ",".join(str(c[1]) for c in cs), ",".join(str(c[1]) for c in lcs), rate / 1000, lrate / 1000))
        # in-band FEC by loss, every fifth packet lost
        speech = voiced + "*(0.15+0.85*max(0\\,sin(2*PI*2.5*t)))"
        for api, ms, br, cx in ((48000, 20, 24000, 9), (16000, 60, 32000, 1), (48000, 40, 32000, 5)):
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", speech + f":s={api}:d=3", "-ac", "1",
                "-f", "f64le", "-c:a", "pcm_f64le", self.p("s.f64"))
            x = f64(self.p("s.f64"))
            got = {}
            for who in ("shravan", "libopus"):
                pk = encode(who, api, 1, [f"bitrate={br}", f"ms={ms}", f"cx={cx}", "fec=1", "loss=20"])
                cs = []
                for fec in (0, 1):
                    run(OPUSREF, "dec", pk, "1", self.p("fec.dec"), str(api), "lose=5", f"fec={fec}")
                    cs.append(follow([x], self.p("fec.dec"), api, 1)[0][0])
                got[who] = cs
            (c0, c1), (l0, l1) = got["shravan"], got["libopus"]
            ok = c1 > c0 and c1 >= 0.8 and c1 >= l1 - 0.03
            self.record("encode", f"opus fec {api // 1000}k {ms}ms cx{cx}, 1 in 5 lost -> libopus", ok,
                        f"concealed {c0:.4f} recovered {c1:.4f} (libopus {l0:.4f} {l1:.4f})")
        # DTX on digital silence between bursts
        bursts = voiced + "*lt(mod(t\\,1)\\,0.35)"
        for api, ms, cbr in ((48000, 20, 0), (48000, 60, 1), (16000, 40, 0)):
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", bursts + f":s={api}:d=4", "-ac", "1",
                "-f", "f64le", "-c:a", "pcm_f64le", self.p("s.f64"))
            x = f64(self.p("s.f64"))
            res = {}
            for who in ("shravan", "libopus"):
                pk = encode(who, api, 1, ["bitrate=20000", f"ms={ms}", f"vbr={1 - cbr}", "dtx=1"])
                sz = sizes(pk)
                res[who] = (sum(1 for k in sz if k == 1), len(sz))
            run(OPUSREF, "dec", self.p("shravan.pk"), "1", self.p("dtx.dec"), str(api))
            y = f64(self.p("dtx.dec"))
            c = follow([x], self.p("dtx.dec"), api, 1)[0][0]
            (nd, npk), (ld, _) = res["shravan"], res["libopus"]
            # (libopus's AUDIO fallback decides DTX by its tonality analysis: no count to compare)
            same = abs(nd - ld) <= 2 or "RESTRICTED_SILK" not in mode[0]
            ok = nd > 0 and same and len(y) == npk * ms * api // 1000 and c >= 0.8
            self.record("encode", f"opus dtx {api // 1000}k {ms}ms {'cbr' if cbr else 'vbr'} -> libopus", ok,
                        f"{nd} of {npk} packets left out (libopus {ld}{'' if 'RESTRICTED_SILK' in mode[0] else ', AUDIO'})"
                        f" corr={c:.4f}")
        # the bandwidth down and up again with the bit rate
        run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", speech + ":s=48000:d=12", "-ac", "1",
            "-f", "f64le", "-c:a", "pcm_f64le", self.p("s.f64"))
        bws = {}
        for who in ("shravan", "libopus"):
            pk = encode(who, 48000, 1, ["cx=9", "rates=24000*50,8000*300,24000*250"])
            d = open(pk, "rb").read()
            seq, p = [], 0
            while p + 4 <= len(d):
                k = struct.unpack_from("<I", d, p)[0]
                seq.append(d[p + 4] >> 5 & 3)
                p += 4 + k
            bws[who] = seq
        def at(seq, start, want):
            return next((i for i in range(start, len(seq)) if seq[i] == want), None)
        sd, ld = at(bws["shravan"], 50, 1), at(bws["libopus"], 50, 1)
        su, lu = (at(bws["shravan"], 350, 2) if sd else None), (at(bws["libopus"], 350, 2) if ld else None)
        ok = None not in (sd, ld, su, lu) and abs(sd - ld) <= 10 and abs(su - lu) <= 10
        self.record("encode", "opus bandwidth down and up with the rate -> libopus", ok,
                    f"mediumband from packet {sd} (libopus {ld}), wideband again from {su} ({lu})")
        print("  note: the Opus layer's reference, libopus " + mode[0])

    def silk_stereo(self, ph, sizes):
        """2.12.2. Stereo against libopus's own encoder forced to two channels
        (OPUS_SET_FORCE_CHANNELS), libopus decoding both: the left a voiced source
        at 110 Hz, the right half of it and half another at 165 Hz (a partly
        correlated image), at every rate, 10 to 60 ms, VBR and CBR, and from 48
        and 24 kHz input. Each channel must follow its input at libopus's lag
        (within a sample), correlate at least 0.8 and within 0.03 of libopus's,
        at libopus's bits as above. At a rate low enough for mid-only coding the
        same against libopus, without the floor; stereo input coded mono against
        libopus forced to one channel, each decoded channel following the
        inputs' average; in-band FEC recovering better than concealment; and DTX
        on bursts over a faint noise floor, its packets left out where libopus's
        are (within two). (On digital silence libopus's Opus layer turns SILK's
        DTX off and decides DTX itself, a packet sooner at 60 ms: that is
        opus_encode's, which silk_encode_opus does not have.)"""
        def voice(f0, seed):
            p = ph.replace("110*t", f"{f0}*t")
            return "+".join(f"{0.3 / k:.4f}*sin(2*PI*{k}*{p})" for k in range(1, 9)) + f"+0.01*random({seed})"
        L = voice(110, 0)
        R = f"0.5*({voice(110, 0)})+0.5*({voice(165, 1)})"

        def make(api, d, gate="", floor=""):
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i",
                f"aevalsrc=({L}){gate}{floor.format(2)}|({R}){gate}{floor.format(3)}:s={api}:d={d}",
                "-f", "f64le", "-c:a", "pcm_f64le", self.p("s.f64"))
            x = f64(self.p("s.f64"))
            return x[0::2], x[1::2]

        def chans(path):
            y = f64(path)
            return y[0::2], y[1::2]

        def corr2(xs, ys, api, ms):
            lag0 = 13 * api // 2000
            return [best_lag_corr(a, b, 1, lag0 + 3 * api // 1000, api // 10, lag0 - 3) for a, b in zip(xs, ys)]

        cases = [(fs, ms, cbr, fs) for fs in (8000, 12000, 16000) for ms in (20, 60) for cbr in (0, 1)]
        cases += [(16000, 10, 0, 16000), (12000, 40, 1, 12000), (16000, 20, 0, 48000), (12000, 40, 0, 24000)]
        for fs, ms, cbr, api in cases:
            br = {8000: 20000, 12000: 28000, 16000: 36000}[fs]
            xs = make(api, 1.5)
            res = {}
            for who in ("shravan", "libopus"):
                pk, back = self.p(who + ".pk"), self.p(who + ".dec")
                if who == "shravan":
                    run(CLI, "silkenc", self.p("s.f64"), str(fs), str(br), "1", str(ms), str(cbr), pk, "opus", "1276",
                        "0", "0", "0", str(api), "2")
                else:
                    run(OPUSREF, "silk", self.p("s.f64"), str(fs), str(br), str(ms), str(cbr), pk, f"api={api}", "ch=2")
                run(OPUSREF, "dec", pk, "2", back, str(api))
                sz = sizes(pk)
                res[who] = (corr2(xs, chans(back), api, ms), sum(sz) * 8 / (len(sz) * ms / 1000), set(sz))
            (cs, rate, sz), (lcs, lrate, lsz) = res["shravan"], res["libopus"]
            ok = all(abs(c[1] - l[1]) <= 1 and c[0] >= 0.8 and c[0] >= l[0] - 0.03 for c, l in zip(cs, lcs))
            ok = ok and (sz == lsz and len(sz) == 1 if cbr else abs(rate / lrate - 1) <= 0.10)
            src = f"{api // 1000}k -> " if api != fs else ""
            self.record("encode", f"silk stereo {src}{fs // 1000}k {ms}ms {'cbr' if cbr else 'vbr'} -> libopus", ok,
                        "corr L=%.4f R=%.4f (libopus %.4f %.4f) lag=%d,%d (%d,%d) %.1f kb/s (%.1f)"
                        % (cs[0][0], cs[1][0], lcs[0][0], lcs[1][0], cs[0][1], cs[1][1], lcs[0][1], lcs[1][1],
                           rate / 1000, lrate / 1000))
        # mid-only coding at a low rate, and stereo input coded mono
        for fs, ms, br, fch in ((16000, 20, 14000, 2), (8000, 40, 9000, 2), (16000, 20, 20000, 1), (12000, 60, 16000, 1)):
            xs = make(fs, 1.5)
            mid = array.array("d", ((a + b) / 2 for a, b in zip(*xs)))
            res = {}
            for who in ("shravan", "libopus"):
                pk, back = self.p(who + ".pk"), self.p(who + ".dec")
                if who == "shravan":
                    run(CLI, "silkenc", self.p("s.f64"), str(fs), str(br), "1", str(ms), "0", pk, "opus", "1276",
                        "0", "0", "0", str(fs), "2", str(fch))
                else:
                    run(OPUSREF, "silk", self.p("s.f64"), str(fs), str(br), str(ms), "0", pk, "ch=2", f"fch={fch}")
                run(OPUSREF, "dec", pk, "2", back, str(fs))
                sz = sizes(pk)
                ref = (mid, mid) if fch == 1 else xs
                res[who] = (corr2(ref, chans(back), fs, ms), sum(sz) * 8 / (len(sz) * ms / 1000))
            (cs, rate), (lcs, lrate) = res["shravan"], res["libopus"]
            ok = all(abs(c[1] - l[1]) <= 1 and c[0] >= l[0] - 0.03 for c, l in zip(cs, lcs))
            ok = ok and abs(rate / lrate - 1) <= 0.10 and (fch == 2 or min(c[0] for c in cs) >= 0.8)
            what = "mono-coded" if fch == 1 else "low-rate"
            self.record("encode", f"silk stereo {what} {fs // 1000}k {ms}ms {br // 1000}k -> libopus", ok,
                        "corr L=%.4f R=%.4f (libopus %.4f %.4f) %.1f kb/s (%.1f)"
                        % (cs[0][0], cs[1][0], lcs[0][0], lcs[1][0], rate / 1000, lrate / 1000))
        # in-band FEC, every fifth packet lost
        for fs, ms in ((16000, 20), (12000, 60), (8000, 40)):
            br = {8000: 24000, 12000: 32000, 16000: 40000}[fs]
            xs = make(fs, 3, "*(0.15+0.85*max(0\\,sin(2*PI*2.5*t)))")
            pk = self.p("fec.pk")
            run(CLI, "silkenc", self.p("s.f64"), str(fs), str(br), "1", str(ms), "0", pk, "opus", "1276", "20", "1",
                "0", str(fs), "2")
            cs = []
            for fec in (0, 1):
                run(OPUSREF, "dec", pk, "2", self.p("fec.dec"), str(fs), "lose=5", f"fec={fec}")
                cs.append(min(c[0] for c in corr2(xs, chans(self.p("fec.dec")), fs, ms)))
            ok = cs[1] > cs[0] and cs[1] >= 0.8
            self.record("encode", f"silk stereo fec {fs // 1000}k {ms}ms, 1 in 5 lost -> libopus", ok,
                        f"concealed {cs[0]:.4f} recovered {cs[1]:.4f} (the worse channel)")
        # DTX on bursts
        for fs, ms, cbr in ((16000, 20, 0), (12000, 40, 1), (8000, 60, 0)):
            br = {8000: 20000, 12000: 28000, 16000: 36000}[fs]
            xs = make(fs, 4, "*lt(mod(t\\,1)\\,0.35)", "+0.0003*random({})")
            res = {}
            for who in ("shravan", "libopus"):
                pk = self.p(who + ".pk")
                if who == "shravan":
                    run(CLI, "silkenc", self.p("s.f64"), str(fs), str(br), "1", str(ms), str(cbr), pk, "opus", "1276",
                        "0", "0", "1", str(fs), "2")
                else:
                    run(OPUSREF, "silk", self.p("s.f64"), str(fs), str(br), str(ms), str(cbr), pk, "dtx=1", "ch=2")
                sz = sizes(pk)
                res[who] = (sum(1 for k in sz if k == 1), len(sz))
            run(OPUSREF, "dec", self.p("shravan.pk"), "2", self.p("dtx.dec"), str(fs))
            ys = chans(self.p("dtx.dec"))
            c = min(cc[0] for cc in corr2(xs, ys, fs, ms))
            (nd, npk), (ld, _) = res["shravan"], res["libopus"]
            ok = nd > 0 and abs(nd - ld) <= 2 and len(ys[0]) == npk * ms * fs // 1000 and c >= 0.8
            self.record("encode", f"silk stereo dtx {fs // 1000}k {ms}ms {'cbr' if cbr else 'vbr'} -> libopus", ok,
                        f"{nd} of {npk} packets left out (libopus {ld}) corr={c:.4f}")

    def silk_api_fec_dtx(self, voiced, sizes):
        """2.12.1. The input at an API rate, resampled to the internal one,
        against libopus's own encoder at the same rates (opus_encode at the API
        rate), as above. In-band FEC with every fifth packet lost, which libopus
        recovers from the next packet's LBRR frames: LBRR frames in at least half
        the packets, and the recovery correlating better than concealment (by
        0.01 to 0.17 here; least at 10 ms, whose gaps conceal well). DTX on
        bursts with 0.65 s of silence between them: 1-byte packets where
        libopus's own encoder leaves its out too (within two), a decode of the
        stream's length, following the input. (At 8 kHz in 10 ms frames neither
        encoder's VAD lets this input go quiet enough for DTX.)"""
        def lbrr(path, ms):
            d = open(path, "rb").read()
            n, p, bit = 0, 0, 1 << (7 - (ms // 20 if ms > 10 else 1))
            while p + 4 <= len(d):
                k = struct.unpack_from("<I", d, p)[0]
                if k > 1 and d[p + 5] & bit:
                    n += 1
                p += 4 + k
            return n
        for api, fs, ms in ((48000, 16000, 20), (48000, 12000, 60), (48000, 8000, 40), (24000, 16000, 40),
                            (24000, 12000, 20), (16000, 12000, 10), (16000, 8000, 20), (12000, 8000, 60)):
            br = {8000: 12000, 12000: 16000, 16000: 20000}[fs]
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", voiced + f":s={api}:d=1.5", "-ac", "1",
                "-f", "f64le", "-c:a", "pcm_f64le", self.p("s.f64"))
            x = f64(self.p("s.f64"))
            res = {}
            for who in ("shravan", "libopus"):
                pk, back = self.p(who + ".pk"), self.p(who + ".dec")
                if who == "shravan":
                    run(CLI, "silkenc", self.p("s.f64"), str(fs), str(br), "1", str(ms), "0", pk, "opus", "1276", "0",
                        "0", "0", str(api))
                else:
                    run(OPUSREF, "silk", self.p("s.f64"), str(fs), str(br), str(ms), "0", pk, f"api={api}")
                run(OPUSREF, "dec", pk, "1", back, str(api))
                lag0 = 13 * api // 2000
                c, lag = best_lag_corr(x, f64(back), 1, lag0 + 3 * api // 1000, api // 10, lag0 - 3)
                sz = sizes(pk)
                res[who] = (c, lag, sum(sz) * 8 / (len(sz) * ms / 1000))
            (c, lag, rate), (lc, llag, lrate) = res["shravan"], res["libopus"]
            ok = abs(lag - llag) <= 1 and c >= 0.8 and c >= lc - 0.03 and abs(rate / lrate - 1) <= 0.10
            self.record("encode", f"silk api {api // 1000}k -> {fs // 1000}k {ms}ms -> libopus", ok,
                        f"corr={c:.4f} (libopus {lc:.4f}) lag={lag} ({llag}) {rate / 1000:.1f} kb/s ({lrate / 1000:.1f})")
        speech = voiced + "*(0.15+0.85*max(0\\,sin(2*PI*2.5*t)))"
        for fs, ms in ((16000, 20), (16000, 60), (12000, 40), (8000, 20), (8000, 10)):
            br = {8000: 16000, 12000: 20000, 16000: 24000}[fs]
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", speech + f":s={fs}:d=3", "-ac", "1",
                "-f", "f64le", "-c:a", "pcm_f64le", self.p("s.f64"))
            x = f64(self.p("s.f64"))
            pk = self.p("fec.pk")
            run(CLI, "silkenc", self.p("s.f64"), str(fs), str(br), "1", str(ms), "0", pk, "opus", "1276", "20", "1", "0")
            cs = []
            for fec in (0, 1):
                run(OPUSREF, "dec", pk, "1", self.p("fec.dec"), str(fs), "lose=5", f"fec={fec}")
                lag0 = 13 * fs // 2000
                cs.append(best_lag_corr(x, f64(self.p("fec.dec")), 1, lag0 + 3, fs // 10, lag0 - 3)[0])
            nl, npk = lbrr(pk, ms), len(sizes(pk))
            ok = nl * 2 >= npk and cs[1] > cs[0] and cs[1] >= 0.8
            self.record("encode", f"silk fec {fs // 1000}k {ms}ms, 1 in 5 lost -> libopus", ok,
                        f"concealed {cs[0]:.4f} recovered {cs[1]:.4f}, LBRR in {nl} of {npk} packets")
        bursts = voiced + "*lt(mod(t\\,1)\\,0.35)"
        for fs, ms, cbr in ((16000, 20, 0), (16000, 60, 1), (12000, 40, 0), (16000, 10, 0), (8000, 20, 1)):
            br = {8000: 12000, 12000: 16000, 16000: 20000}[fs]
            run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", bursts + f":s={fs}:d=4", "-ac", "1",
                "-f", "f64le", "-c:a", "pcm_f64le", self.p("s.f64"))
            x = f64(self.p("s.f64"))
            res = {}
            for who in ("shravan", "libopus"):
                pk = self.p(who + ".pk")
                if who == "shravan":
                    run(CLI, "silkenc", self.p("s.f64"), str(fs), str(br), "1", str(ms), str(cbr), pk, "opus", "1276",
                        "0", "0", "1")
                else:
                    run(OPUSREF, "silk", self.p("s.f64"), str(fs), str(br), str(ms), str(cbr), pk, "dtx=1")
                sz = sizes(pk)
                res[who] = (sum(1 for k in sz if k == 1), len(sz))
            run(OPUSREF, "dec", self.p("shravan.pk"), "1", self.p("dtx.dec"), str(fs))
            y = f64(self.p("dtx.dec"))
            lag0 = 13 * fs // 2000
            c = best_lag_corr(x, y, 1, lag0 + 3, fs // 10, lag0 - 3)[0]
            (nd, npk), (ld, _) = res["shravan"], res["libopus"]
            ok = nd > 0 and abs(nd - ld) <= 2 and len(y) == npk * ms * fs // 1000 and c >= 0.8
            self.record("encode", f"silk dtx {fs // 1000}k {ms}ms {'cbr' if cbr else 'vbr'} -> libopus", ok,
                        f"{nd} of {npk} packets left out (libopus {ld}) corr={c:.4f}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--only", help="comma-separated: wav,aiff,flac,alac,mp3,aac,gapless,opus,encoders,silk,opusenc,celt")
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args()
    groups = ["wav", "aiff", "flac", "alac", "mp3", "aac", "gapless", "opus", "encoders", "silk", "opusenc", "celt"]
    if args.only:
        groups = [g for g in groups if g in args.only.split(",")]
    for tool in (CLI, OPUSREF):
        if not os.path.exists(tool):
            sys.exit(f"missing {tool}: run scripts/interop.sh")
    with tempfile.TemporaryDirectory() as tmp:
        sw = Sweep(tmp, args.verbose)
        for g in groups:
            print(f"== {g}")
            getattr(sw, g)()
        bad = [r for r in sw.results if not r[2]]
        print(f"\ninterop: {len(sw.results) - len(bad)} of {len(sw.results)} passed")
        sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
