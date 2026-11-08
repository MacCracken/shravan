#!/usr/bin/env python3
"""Generate src/interop_fixtures.cyr: real encoder output with reference decodes.

shravan's round-trip tests decode what shravan encoded, so they cannot see a bug the
encoder and decoder share, or a path shravan's encoder never takes. 2.9.2 found FLAC
LPC, ALAC, AAC at most sample rates and MP3 count1 table B all broken behind green
suites that way. These fixtures come from ffmpeg, flac and lame, and the expected
values come from ffmpeg's decoders, so src/interop_tests.cyr compares shravan against
an independent reference.

Each fixture asserts it exercises the path it exists for. If a tool changes and stops
producing that path, this script fails rather than writing a test that proves nothing.

The MPEG audio conformance excerpts come from the ISO/IEC 11172-4 and 13818-4
bitstreams with their reference decodes, as redistributed in minimp3's test vectors:
set MINIMP3_VECTORS to a checkout's vectors/ directory, or the script fetches them
(git, sparse). Needs ffmpeg (with libmp3lame and libopus), flac, a C compiler and the libopus
headers (the Opus fixtures come from scripts/interop/opusref.c, which records
libopus's own decode of the exact packets). That libopus must be a default build, not
a deep-PLC one: set OPUS_PREFIX to one made by scripts/interop/build-libopus.sh if
the system's is deep-PLC (the script says so and stops). The output is checked in, so running
the tests needs none of them. Regenerate with:

    ./scripts/gen-interop-fixtures.py
"""
import array
import base64
import json
import os
import struct
import subprocess
import sys
import tempfile
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "src", "interop_fixtures.cyr")
FNV_OFFSET = 1469598103934665603
FNV_PRIME = 0x100000001B3
MASK64 = (1 << 64) - 1

# Bursts at different times in each channel: transients force short blocks, and
# different times force the channels onto independent windows (common_window = 0).
STEREO_TRANSIENT = (
    "aevalsrc=0.45*exp(-30*mod(t\\,0.11))*sin(2*PI*(%d/9)*t)"
    "+0.05*exp(-90*mod(t+0.04\\,0.09))*random(0)"
    "|0.4*exp(-25*mod(t+0.05\\,0.13))*sin(2*PI*(%d/7)*t):s=%d:d=%s"
)
TONE_NOISE = (
    "aevalsrc=0.4*sin(2*PI*440*t)+0.05*random(0)"
    "|0.3*sin(2*PI*660*t)+0.05*random(1):s=%d:d=%s"
)


def run(*args, check=True):
    return subprocess.run(args, check=check, capture_output=True)


def ffmpeg_make(out, lavfi, *codec_args):
    run("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", lavfi, *codec_args, out)


def reference_f64(path, *extra):
    """Decode with ffmpeg, keeping the file's own priming so frames line up."""
    keep = ("-ignore_editlist", "1") if path.endswith((".m4a", ".mov")) else ()
    p = run("ffmpeg", "-v", "error", *keep, *extra, "-i", path,
            "-f", "f64le", "-acodec", "pcm_f64le", "-")
    a = array.array("d")
    a.frombytes(p.stdout[: len(p.stdout) // 8 * 8])
    return a


def fnv_hash(ints):
    h = FNV_OFFSET
    for v in ints:
        h = ((h ^ (v & 0xFFFFFFFF)) * FNV_PRIME) & MASK64
    return h


def b64_literal(name, data):
    return 'var %s = "%s";\n' % (name, base64.b64encode(data).decode("ascii"))


def exact_fixture(name, path, bps):
    """Lossless: every sample must match, so a hash of all of them is enough."""
    ref = reference_f64(path)
    scale = 1 << (bps - 1)
    ints = [int(round(x * scale)) for x in ref]
    text = b64_literal("_FX_%s_B64" % name, open(path, "rb").read())
    text += "var _FX_%s_COUNT = %d;\n" % (name, len(ints))
    text += "var _FX_%s_HASH = %d;\n" % (name, fnv_hash(ints))
    return text


# ---- ID3v2 tags with compressed frames ----
# No installed tool writes them, so they are built here byte for byte from the
# specs (id3v2.3.0 3.3.1, id3v2.4.0-structure 4.1.2) and accepted only if a
# reference reads them back: ffprobe for v2.4 (ffmpeg ignores the v2.3 frame
# flags) and, when installed, mutagen for v2.3.
ID3_TITLE = "Compressed Title " + "x" * 40
ID3_ARTIST = "Grouped Artist"
ID3_ALBUM = "Plain Album"
ID3_COMMENT = "Compressed comment"


def _syncsafe(n):
    return bytes([(n >> 21) & 0x7F, (n >> 14) & 0x7F, (n >> 7) & 0x7F, n & 0x7F])


def _unsync(b):
    out = bytearray()
    for i, c in enumerate(b):
        out.append(c)
        if c == 0xFF and (i + 1 == len(b) or b[i + 1] == 0 or b[i + 1] >= 0xE0):
            out.append(0)
    return bytes(out)


def _v23_frame(fid, body, compress=False, encrypt=False, group=None):
    flags, extra = 0, b""
    if compress:
        flags |= 0x80
        extra += struct.pack(">I", len(body))
        body = zlib.compress(body)
    if encrypt:
        flags |= 0x40
        extra += b"\x80"
    if group is not None:
        flags |= 0x20
        extra += bytes([group])
    data = extra + body
    return fid + struct.pack(">I", len(data)) + bytes([0, flags]) + data


def _v24_frame(fid, body, compress=False, unsync=False, encrypt=False, group=None):
    flags, extra, raw = 0, b"", len(body)
    if group is not None:
        flags |= 0x40
        extra += bytes([group])
    if encrypt:
        flags |= 0x04
        extra += b"\x80"
    if compress:
        flags |= 0x08 | 0x01
        body = zlib.compress(body)
    if unsync:
        flags |= 0x02
        body = _unsync(body)
    if flags & 0x01:
        extra += _syncsafe(raw)
    data = extra + body
    return fid + _syncsafe(len(data)) + bytes([0, flags]) + data


def _id3_tag(version, frames, flags=0):
    body = b"".join(frames) + b"\0" * 32
    return b"ID3" + bytes([version, 0, flags]) + _syncsafe(len(body)) + body


def id3_fixtures(tmp):
    """The tags, checked against the references, as fixture text."""
    lat = lambda t: b"\x00" + t.encode("latin-1")
    utf = lambda t: b"\x03" + t.encode("utf-8")
    comm = lambda t: b"\x03eng\x00" + t.encode("utf-8")
    v23 = _id3_tag(3, [
        _v23_frame(b"TIT2", lat(ID3_TITLE), compress=True),
        _v23_frame(b"TPE1", lat(ID3_ARTIST), group=7),
        _v23_frame(b"TCON", lat("Encrypted"), encrypt=True),
        _v23_frame(b"TALB", lat(ID3_ALBUM)),
    ])
    v24 = _id3_tag(4, [
        _v24_frame(b"TIT2", utf(ID3_TITLE), compress=True),
        _v24_frame(b"TPE1", utf(ID3_ARTIST), group=7),
        _v24_frame(b"TCON", utf("Encrypted"), encrypt=True),
        _v24_frame(b"TALB", utf(ID3_ALBUM), compress=True, unsync=True),
        _v24_frame(b"COMM", comm(ID3_COMMENT), compress=True),
    ])
    # v2.2 with the header's compression bit: the spec says ignore the tag.
    v22_body = b"TT2" + bytes([0, 0, 6]) + lat("Title") + b"\0" * 6
    v22 = b"ID3" + bytes([2, 0, 0x40]) + _syncsafe(len(v22_body)) + v22_body
    bare = os.path.join(tmp, "bare.mp3")
    ffmpeg_make(bare, "sine=f=440:d=0.1", "-ac", "1", "-c:a", "libmp3lame", "-b:a", "32k",
                "-id3v2_version", "0", "-write_xing", "0", "-map_metadata", "-1")
    audio = open(bare, "rb").read()
    tagged = os.path.join(tmp, "v24.mp3")
    open(tagged, "wb").write(v24 + audio)
    probe = run("ffprobe", "-v", "error", "-show_entries", "format_tags", "-of", "json", tagged).stdout
    tags = json.loads(probe)["format"]["tags"]
    require(tags.get("title") == ID3_TITLE, "ffprobe reads the v2.4 compressed title")
    require(tags.get("album") == ID3_ALBUM, "ffprobe reads the v2.4 compressed + unsynchronised album")
    require(tags.get("comment") == ID3_COMMENT, "ffprobe reads the v2.4 compressed comment")
    try:
        from mutagen.id3 import ID3
    except ImportError:
        print("note: mutagen not installed; the v2.3 compressed frame is checked against the spec only")
    else:
        tagged23 = os.path.join(tmp, "v23.mp3")
        open(tagged23, "wb").write(v23 + audio)
        require(str(ID3(tagged23)["TIT2"]) == ID3_TITLE, "mutagen reads the v2.3 compressed title")
    text = b64_literal("_FX_ID3V23Z_B64", v23)
    text += b64_literal("_FX_ID3V24Z_B64", v24)
    text += b64_literal("_FX_ID3V22Z_B64", v22)
    return text


# ---- hostile MP4s (the 2.9.3 ALAC/MP4 bounds audit) ----
# Built box by box. Each stands for a finding: the decoder must refuse it (or, for
# the 64-bit trak, decode it as ffmpeg does).
def _mbox(t, payload):
    return struct.pack(">I4s", 8 + len(payload), t) + payload


def _mfull(t, payload):
    return _mbox(t, b"\0\0\0\0" + payload)


def _mp4_walk(d, start, end, out):
    pos = start
    while pos + 8 <= end:
        size, t = struct.unpack(">I4s", d[pos:pos + 8])
        hdr = 8
        if size == 1:
            size = struct.unpack(">Q", d[pos + 8:pos + 16])[0]
            hdr = 16
        if size == 0:
            size = end - pos
        out.append((t, pos, size, hdr))
        if t in (b"moov", b"trak", b"mdia", b"minf", b"stbl"):
            _mp4_walk(d, pos + hdr, pos + size, out)
        pos += size
    return out


def _mp4_box(d, t):
    return [b for b in _mp4_walk(d, 0, len(d), []) if b[0] == t][0]


def _mp4_build(entry, stsz, stsc, stco_of, mdat):
    """ftyp + moov(trak/mdia/hdlr/minf/stbl) + mdat; stco_of(mdat payload offset)."""
    ftyp = _mbox(b"ftyp", b"M4A \0\0\0\0M4A mp42")
    hdlr = _mfull(b"hdlr", b"\0" * 4 + b"soun" + b"\0" * 13)
    stsd = _mfull(b"stsd", struct.pack(">I", 1) + entry)

    def moov(off):
        stbl = _mbox(b"stbl", stsd + _mfull(b"stsz", stsz) + _mfull(b"stsc", stsc)
                     + _mfull(b"stco", stco_of(off)))
        return _mbox(b"moov", _mbox(b"trak", _mbox(b"mdia", hdlr + _mbox(b"minf", stbl))))

    off = len(ftyp) + len(moov(0)) + 8
    return ftyp + moov(off) + _mbox(b"mdat", mdat)


def mp4_audit_fixtures(p):
    text = ""
    # F1: a constant sample size of 1 with as many samples as bytes. The sample
    # tables cost 16 bytes a sample, 16x the file; refused since 2.9.3.
    cookie = struct.pack(">IBBBBBBHIII", 1, 0, 16, 40, 10, 14, 1, 255, 0, 0, 44100)
    alac = (b"\0" * 6 + struct.pack(">H", 1) + struct.pack(">HHI", 0, 0, 0)
            + struct.pack(">HHHH", 1, 16, 0, 0) + struct.pack(">I", 44100 << 16)
            + _mfull(b"alac", cookie))
    n = 4096
    const1 = _mp4_build(_mbox(b"alac", alac), struct.pack(">II", 1, n),
                        struct.pack(">IIII", 1, 1, n, 1), lambda o: struct.pack(">II", 1, o),
                        b"\xe0" * n)
    text += b64_literal("_FX_MP4CONST1_B64", const1)

    # F2: AAC samples whose chunks all point at the same access unit. Every AU is in
    # bounds, but their sizes sum to far more than the file, and the ADTS staging
    # copy was that sum. Refused since 2.9.3.
    src = open(p("aac96.m4a"), "rb").read()
    t, pos, size, hdr = _mp4_box(src, b"stsz")
    sd, cnt = struct.unpack(">II", src[pos + hdr + 4:pos + hdr + 12])
    s0 = struct.unpack(">I", src[pos + hdr + 12:pos + hdr + 16])[0] if sd == 0 else sd
    t, pos, size, hdr = _mp4_box(src, b"stco")
    o0 = struct.unpack(">I", src[pos + hdr + 8:pos + hdr + 12])[0]
    au = src[o0:o0 + s0]
    t, pos, size, hdr = _mp4_box(src, b"stsd")
    q = pos + hdr + 8
    entry = src[q:q + struct.unpack(">I", src[q:q + 4])[0]]
    k = 64
    overlap = _mp4_build(entry, struct.pack(">II", 0, k) + struct.pack(">I", s0) * k,
                         struct.pack(">IIII", 1, 1, 1, 1),
                         lambda o: struct.pack(">I", k) + struct.pack(">I", o) * k, au)
    text += b64_literal("_FX_MP4OVERLAP_B64", overlap)

    # F4: an access unit too large for the ADTS header the bridge builds (13-bit
    # length; AAC-LC at two channels never needs more than 1536 bytes).
    big = 8190
    bigau = _mp4_build(entry, struct.pack(">II", 0, 1) + struct.pack(">I", big),
                       struct.pack(">IIII", 1, 1, 1, 1), lambda o: struct.pack(">II", 1, o),
                       (au + b"\0" * big)[:big])
    text += b64_literal("_FX_MP4BIGAU_B64", bigau)

    # F4: AudioSpecificConfig channel configuration 7 (eight channels), which the
    # ADTS bridge wrote as configuration 0.
    d = bytearray(src)
    t, pos, size, hdr = _mp4_box(bytes(d), b"stsd")
    e = bytes(d).find(b"esds", pos)
    tag5 = bytes(d).find(b"\x05", e + 8)
    asc = tag5 + 1
    while d[asc] & 0x80:
        asc += 1
    asc += 1
    require(((d[asc] >> 3) & 0x1F) == 2, "ASC audio object type 2")
    d[asc + 1] = (d[asc + 1] & 0x87) | (7 << 3)
    text += b64_literal("_FX_MP4ASCCH7_B64", bytes(d))

    # F4: an explicit sampling rate (index 15) that has no ADTS index, 22000 Hz.
    # The bridge used to write 44.1 kHz in its place. Three bytes go into the ASC,
    # so every enclosing descriptor, box and the chunk offsets grow with it.
    d = bytearray(src)
    boxes = _mp4_walk(bytes(d), 0, len(d), [])
    t, pos, size, hdr = _mp4_box(bytes(d), b"stsd")
    e = bytes(d).find(b"esds", pos)
    descs = []
    q = e + 8
    for tag in (3, 4, 5):
        q = bytes(d).find(bytes([tag]), q)
        descs.append(q)
        q += 1
        while d[q] & 0x80:
            q += 1
        q += 1
        if tag == 3:
            q += 3          # ES_ID(2) + flags(1)
        if tag == 4:
            q += 13         # objectType, streamType, bufferSize, maxBitrate, avgBitrate
    asc = q
    aot, idx, ch = d[asc] >> 3, ((d[asc] & 7) << 1) | (d[asc + 1] >> 7), (d[asc + 1] >> 3) & 15
    require(aot == 2, "ASC audio object type 2")
    rest = d[asc + 1] & 7
    bits = (2 << 35) | (15 << 31) | (22000 << 7) | (ch << 3) | rest
    old_asc = bytes(d[asc:asc + 2])
    new_asc = bits.to_bytes(5, "big")
    d[asc:asc + 2] = new_asc
    grow = len(new_asc) - len(old_asc)
    for dp in descs:            # each descriptor's last length byte
        lp = dp + 1
        while d[lp] & 0x80:
            lp += 1
        d[lp] += grow
    for t2, bp, bsz, bhdr in boxes:
        if bp < asc < bp + bsz:
            if bhdr == 8:
                d[bp:bp + 4] = struct.pack(">I", struct.unpack(">I", d[bp:bp + 4])[0] + grow)
    # The sample entry and the esds sit below the walk; grow them too.
    ent = pos + hdr + 8
    d[ent:ent + 4] = struct.pack(">I", struct.unpack(">I", d[ent:ent + 4])[0] + grow)
    d[e - 4:e] = struct.pack(">I", struct.unpack(">I", d[e - 4:e])[0] + grow)
    t2, sp, ssz, shdr = _mp4_box(bytes(d), b"stco")
    nq = sp + shdr + 4
    for i in range(struct.unpack(">I", d[nq:nq + 4])[0]):
        o = struct.unpack(">I", d[nq + 4 + 4 * i:nq + 8 + 4 * i])[0]
        if o > asc:
            d[nq + 4 + 4 * i:nq + 8 + 4 * i] = struct.pack(">I", o + grow)
    open(p("rate22000.m4a"), "wb").write(d)
    # The container is sound (ffprobe reads 22000 Hz from it), and ffmpeg's AAC
    # decoder refuses the explicit rate too ("invalid sampling rate index 15").
    probe = run("ffprobe", "-hide_banner", p("rate22000.m4a"), check=False).stderr
    require(b"22000 Hz, 2 channels" in probe, "ffprobe reads the explicit 22000 Hz rate")
    dec = run("ffmpeg", "-v", "error", "-i", p("rate22000.m4a"), "-f", "null", "-", check=False)
    require(b"invalid sampling rate index" in dec.stderr, "ffmpeg's decoder refuses the explicit rate")
    text += b64_literal("_FX_MP4RATE_B64", bytes(d))

    # F3: the audio trak with a 64-bit largesize header. ffmpeg decodes it; the
    # track search used to step through it a byte at a time and miss it.
    ffmpeg_make(p("small.m4a"), "sine=f=440:d=0.02:sample_rate=8000", "-ac", "1",
                "-c:a", "alac", "-sample_fmt", "s16p")
    d = bytearray(open(p("small.m4a"), "rb").read())
    boxes = _mp4_walk(bytes(d), 0, len(d), [])
    t, tp, tsz, thdr = [b for b in boxes if b[0] == b"trak"][0]
    anc = [b for b in boxes if b[1] < tp < b[1] + b[2]]
    new = d[:tp] + struct.pack(">I4sQ", 1, b"trak", tsz + 8) + d[tp + 8:]
    for a in anc:
        asz = struct.unpack(">I", new[a[1]:a[1] + 4])[0]
        new[a[1]:a[1] + 4] = struct.pack(">I", asz + 8)
    t, sp, ssz, shdr = _mp4_box(bytes(new), b"stco")
    nq = sp + shdr + 4
    for i in range(struct.unpack(">I", new[nq:nq + 4])[0]):
        o = struct.unpack(">I", new[nq + 4 + 4 * i:nq + 8 + 4 * i])[0]
        if o > tp:
            new[nq + 4 + 4 * i:nq + 8 + 4 * i] = struct.pack(">I", o + 8)
    open(p("largetrak.m4a"), "wb").write(new)
    text += exact_fixture("MP4LARGETRAK", p("largetrak.m4a"), 16)
    return text


def opus_fixture(tmp, name, config, channels, packets, step, *opts, want=None):
    """Raw Opus packets from libopus with libopus's decode of them as the reference.

    Packets carry a u32le length each. `want` checks the configs and channel mix the
    stream actually holds, since libopus may not produce exactly what was forced.
    """
    pk, ref = os.path.join(tmp, name + ".pk"), os.path.join(tmp, name + ".ref")
    out = run(os.path.join(tmp, "opusref"), "gen", str(config), str(channels),
              str(packets), pk, ref, *opts).stdout.decode()
    if want:
        want(out)
    a = array.array("d")
    a.frombytes(open(ref, "rb").read())
    picks = [int(round(a[i] * 32768)) for i in range(0, len(a), step)]
    text = b64_literal("_FX_%s_B64" % name, open(pk, "rb").read())
    text += "var _FX_%s_COUNT = %d;\n" % (name, len(a))
    text += "var _FX_%s_STEP = %d;\n" % (name, step)
    text += 'var _FX_%s_REF = "%s";\n' % (name, ",".join(map(str, picks)))
    return text, out


def opus_packets(path):
    data = open(path, "rb").read()
    pos, out = 0, []
    while pos + 4 <= len(data):
        n = int.from_bytes(data[pos:pos + 4], "little")
        out.append(data[pos + 4:pos + 4 + n])
        pos += 4 + n
    return out


def silk_lbrr(packet, channels, nframes):
    """Any LBRR flag set in a code-0 SILK/hybrid packet? Decodes the header's VAD
    and LBRR flag bits with the RFC 6716 range decoder (ec_dec_bit_logp(1))."""
    if packet[0] & 3:
        return False
    buf, pos = packet[1:], [0]

    def byte():
        b = buf[pos[0]] if pos[0] < len(buf) else 0
        pos[0] += 1
        return b

    st = {"rng": 128, "rem": byte()}
    st["val"] = 127 - (st["rem"] >> 1)

    def normalize():
        while st["rng"] <= 1 << 23:
            st["rng"] <<= 8
            sym = st["rem"]
            st["rem"] = byte()
            sym = ((sym << 8) | st["rem"]) >> 1
            st["val"] = ((st["val"] << 8) + (255 & ~sym)) & ((1 << 31) - 1)

    normalize()

    def bit():
        s = st["rng"] >> 1
        ret = st["val"] < s
        if not ret:
            st["val"] -= s
        st["rng"] = s if ret else st["rng"] - s
        normalize()
        return ret

    flags = []
    for _ in range(channels):
        for _ in range(nframes):
            bit()               # VAD
        flags.append(bit())     # LBRR
    return any(flags)


def configs_of(out):
    """Parse opusref's "configs: 9 x24 15 x24 | stereo 30/60" line."""
    head, _, rest = out.partition("|")
    nums = head.split(":")[1].split()
    cfgs = {int(nums[i]): int(nums[i + 1][1:]) for i in range(0, len(nums), 2)}
    stereo = int(rest.split("stereo")[1].split("/")[0])
    return cfgs, stereo


def mpeg_frames(data):
    """(offset, length, output samples) of each MPEG audio frame, following the first
    header's version, layer and rate. Free format learns its length from the next
    matching header."""
    BR = {(3, 3): [0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448],
          (3, 2): [0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384],
          (3, 1): [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320],
          (2, 3): [0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256],
          (2, 2): [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
          (2, 1): [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160]}
    SR = {3: [44100, 48000, 32000], 2: [22050, 24000, 16000], 0: [11025, 12000, 8000]}

    def hdr(p):
        if p + 4 > len(data) or data[p] != 0xFF or (data[p + 1] & 0xE0) != 0xE0:
            return None
        ver, lay = (data[p + 1] >> 3) & 3, (data[p + 1] >> 1) & 3
        bri, sri = data[p + 2] >> 4, (data[p + 2] >> 2) & 3
        if ver == 1 or lay == 0 or bri == 15 or sri == 3:
            return None
        return ver, lay, bri, sri, (data[p + 2] >> 1) & 1, (data[p + 3] >> 6) & 3

    out, pos, free = [], 0, 0
    while pos + 4 <= len(data):
        h = hdr(pos)
        if not h:
            pos += 1
            continue
        ver, lay, bri, sri, pad, mode = h
        key = (3 if ver == 3 else 2, lay)
        sr = SR[ver][sri]
        if lay == 3:
            spf, unit = 384, 4
        else:
            spf, unit = (1152 if lay == 2 or ver == 3 else 576), 1
        if bri == 0:
            if not free:
                q = pos + 4
                while q + 4 <= len(data):
                    g = hdr(q)
                    if g and g[:2] == (ver, lay) and g[3] == sri and g[2] == 0:
                        free = q - pos - pad * unit
                        break
                    q += 1
            length = free + pad * unit
        elif lay == 3:
            length = (12 * BR[key][bri] * 1000 // sr + pad) * 4
        else:
            length = spf * BR[key][bri] * 1000 // (8 * sr) + pad
        out.append((pos, length, spf * (1 if mode == 3 else 2), mode))
        pos += length
    return out


def iso_excerpt(vectors, name, stem, first, count, skip, step):
    """A run of frames from an ISO conformance bitstream, judged against the ISO
    reference decode. `first` must not lean on the bit reservoir before it, and the
    first `skip` frames are not compared (the filterbanks start without history)."""
    data = open(os.path.join(vectors, stem + ".bit"), "rb").read()
    ref = array.array("h")
    raw = open(os.path.join(vectors, stem + ".pcm"), "rb").read()
    ref.frombytes(raw[: len(raw) // 2 * 2])
    frames = mpeg_frames(data)
    require(len(frames) >= first + count, "ISO excerpt %s has the frames" % stem)
    # The reference stores each frame in its own layout; shravan outputs one, stereo
    # if any frame of the stream is, with mono frames on both channels.
    stereo = any(f[3] != 3 for f in frames[first:first + count])
    out, cursor = [], sum(f[2] for f in frames[:first])
    widths = []
    for f in frames[first:first + count]:
        seg = ref[cursor:cursor + f[2]]
        cursor += f[2]
        mono = f[3] == 3
        if stereo and mono:
            seg = [v for v in seg for _ in (0, 1)]
        out.extend(seg)
        widths.append(len(seg))
    off = sum(widths[:skip])
    total = len(out)
    a, b = frames[first][0], frames[first + count - 1][0] + frames[first + count - 1][1]
    picks = [out[i] for i in range(off, total, step)]
    text = b64_literal("_FX_%s_B64" % name, data[a:b])
    text += "var _FX_%s_COUNT = %d;\n" % (name, total)
    text += "var _FX_%s_OFF = %d;\n" % (name, off)
    text += "var _FX_%s_STEP = %d;\n" % (name, step)
    text += 'var _FX_%s_REF = "%s";\n' % (name, ",".join(map(str, picks)))
    return text


def minimp3_vectors(tmp):
    given = os.environ.get("MINIMP3_VECTORS")
    if given:
        return given
    dst = os.path.join(tmp, "minimp3")
    run("git", "clone", "-q", "--depth", "1", "--filter=blob:none", "--sparse",
        "https://github.com/lieff/minimp3.git", dst)
    run("git", "-C", dst, "sparse-checkout", "set", "vectors")
    return os.path.join(dst, "vectors")


def lossy_fixture(name, path, step, skip_frames=0, extra=()):
    """Lossy: compare every step-th sample as a 16-bit value, allowing for rounding.

    ffmpeg's float decoders and shravan's f64 ones agree to about 1e-6, far inside
    the tolerance. Random decode damage lands on nearly every sampled point.
    """
    ref = reference_f64(path, *extra)
    picks = [int(round(ref[i] * 32768)) for i in range(0, len(ref), step)]
    text = b64_literal("_FX_%s_B64" % name, open(path, "rb").read())
    text += "var _FX_%s_COUNT = %d;\n" % (name, len(ref))
    text += "var _FX_%s_STEP = %d;\n" % (name, step)
    text += 'var _FX_%s_REF = "%s";\n' % (name, ",".join(map(str, picks)))
    return text


def ogg_crc(data):
    """Ogg page CRC-32 (polynomial 0x04C11DB7, no reflection)."""
    crc = 0
    for b in data:
        crc ^= b << 24
        for _ in range(8):
            crc = ((crc << 1) ^ 0x04C11DB7) & 0xFFFFFFFF if crc & 0x80000000 else (crc << 1) & 0xFFFFFFFF
    return crc


def flac_analysis(path):
    return run("flac", "--analyze", "--stdout", "-s", path).stdout.decode()


def require(cond, what):
    if not cond:
        sys.exit("fixture does not exercise its path: " + what)


def mdat_payload(path):
    """First sample of a one-chunk MP4: the bytes after the mdat header."""
    data = open(path, "rb").read()
    pos = 0
    while pos + 8 <= len(data):
        size = int.from_bytes(data[pos:pos + 4], "big")
        kind = data[pos + 4:pos + 8]
        if kind == b"mdat":
            return data[pos + 8:]
        pos += size
    sys.exit("no mdat in " + path)


def alac_shift_bits(path):
    """bytes_shifted from the first ALAC frame header: tag(3) instance(4) unused(12)
    partial(1) shifted(2) escape(1)."""
    head = int.from_bytes(mdat_payload(path)[:4], "big")
    return (head >> (32 - 22)) & 3


def adts_first_frames(path, wanted):
    """Walk ADTS frames, reporting (frame count, how many start a CPE with
    common_window = 0, how many are EIGHT_SHORT)."""
    data = open(path, "rb").read()
    pos = frames = independent = 0
    while pos + 7 <= len(data):
        length = ((data[pos + 3] & 3) << 11) | (data[pos + 4] << 3) | (data[pos + 5] >> 5)
        bits = int.from_bytes(data[pos + 7:pos + 9], "big")
        element = bits >> 13
        common = (bits >> 8) & 1
        if element == 1 and common == 0:
            independent += 1
        frames += 1
        pos += length
    require(frames >= wanted, "too few ADTS frames")
    return frames, independent


def main():
    parts = [
        "# interop_fixtures.cyr -- real encoder output with reference decodes.\n",
        "# Generated by scripts/gen-interop-fixtures.py. Do not edit.\n\n",
    ]
    with tempfile.TemporaryDirectory() as tmp:
        p = lambda f: os.path.join(tmp, f)

        # FLAC: LPC subframes and stereo decorrelation, which shravan's Fixed-only
        # encoder never writes.
        ffmpeg_make(p("s16.wav"), TONE_NOISE % (44100, "0.12"), "-c:a", "pcm_s16le")
        run("flac", "-s", "-f", "--best", p("s16.wav"), "-o", p("flac16.flac"))
        text = flac_analysis(p("flac16.flac"))
        require("type=LPC" in text, "FLAC 16-bit LPC")
        require("MID_SIDE" in text or "LEFT_SIDE" in text or "RIGHT_SIDE" in text,
                "FLAC stereo decorrelation")
        parts.append(exact_fixture("FLAC16", p("flac16.flac"), 16))

        ffmpeg_make(p("s8.wav"), TONE_NOISE % (8000, "0.2"), "-ac", "1", "-c:a", "pcm_u8")
        run("flac", "-s", "-f", "--best", p("s8.wav"), "-o", p("flac8.flac"))
        require("type=LPC" in flac_analysis(p("flac8.flac")), "FLAC 8-bit LPC")
        parts.append(exact_fixture("FLAC8", p("flac8.flac"), 8))

        # FLAC at 32 bits (RFC 9639's sample-size code 7, reserved before it; the
        # stereo side channel needs 33 bits).
        ffmpeg_make(p("s32.wav"), TONE_NOISE % (48000, "0.02"), "-c:a", "pcm_s32le")
        run("flac", "-s", "-f", "-8", p("s32.wav"), "-o", p("flac32.flac"))
        parts.append(exact_fixture("FLAC32", p("flac32.flac"), 32))

        # A seek point on the last frame, which is shorter than the rest. A
        # fixed-blocksize frame header counts frames, so the position is the frame
        # number times STREAMINFO's block size; until 2.9.3 flac_decode_range
        # multiplied by the frame's own size and returned nothing past that point.
        ffmpeg_make(p("seek.wav"), "sine=f=440:d=1.3:sample_rate=8000", "-ac", "1",
                    "-c:a", "pcm_s16le")
        run("flac", "-s", "-f", "--no-padding", "--blocksize=4096", "-S", "8192",
            p("seek.wav"), "-o", p("seek.flac"))
        listing = run("metaflac", "--list", "--block-type=SEEKTABLE", p("seek.flac")).stdout
        require(b"sample_number=8192" in listing and b"frame_samples=2208" in listing,
                "FLAC seek point on a short last frame")
        parts.append(exact_fixture("FLACSEEK", p("seek.flac"), 16))

        # AIFF-C with big-endian float (fl32), which decoded as integers before 2.9.3,
        # and one with a compression shravan does not decode (ulaw), which must be
        # refused rather than read as PCM.
        ffmpeg_make(p("f32.aiff"), TONE_NOISE % (44100, "0.01"), "-c:a", "pcm_f32be")
        require(b"fl32" in open(p("f32.aiff"), "rb").read()[:256].lower(), "AIFF-C fl32")
        parts.append(exact_fixture("AIFCF32", p("f32.aiff"), 32))
        ffmpeg_make(p("ulaw.aiff"), TONE_NOISE % (8000, "0.01"), "-ac", "1", "-c:a", "pcm_mulaw")
        require(b"ulaw" in open(p("ulaw.aiff"), "rb").read()[:256], "AIFF-C ulaw")
        parts.append(b64_literal("_FX_AIFCULAW_B64", open(p("ulaw.aiff"), "rb").read()))

        # WAV and AIFF as ffmpeg writes them, for the streaming decoders (which must
        # match the one-shot ones however the input is split) and the header parser:
        # a LIST chunk before `data`, IEEE float, WAVE_FORMAT_EXTENSIBLE (ffmpeg
        # writes it past two channels), and a WAV written to a pipe, whose sizes are
        # 0xFFFFFFFF because ffmpeg could not seek back to fill them in.
        ffmpeg_make(p("s24.wav"), TONE_NOISE % (44100, "0.02"), "-c:a", "pcm_s24le")
        require(b"LIST" in open(p("s24.wav"), "rb").read()[:256], "WAV LIST chunk")
        parts.append(exact_fixture("WAV24", p("s24.wav"), 24))
        ffmpeg_make(p("f32.wav"), TONE_NOISE % (44100, "0.01"), "-c:a", "pcm_f32le")
        parts.append(exact_fixture("WAVF32", p("f32.wav"), 32))
        ffmpeg_make(p("ext.wav"), TONE_NOISE % (48000, "0.01"), "-ac", "3", "-c:a", "pcm_s16le")
        require(open(p("ext.wav"), "rb").read()[20:22] == b"\xfe\xff", "WAVE_FORMAT_EXTENSIBLE")
        parts.append(exact_fixture("WAVEXT", p("ext.wav"), 16))
        piped = run("ffmpeg", "-v", "error", "-f", "lavfi", "-i", TONE_NOISE % (22050, "0.02"),
                    "-ac", "1", "-c:a", "pcm_s16le", "-f", "wav", "-").stdout
        require(piped[4:8] == b"\xff\xff\xff\xff", "piped WAV with unknown sizes")
        open(p("pipe.wav"), "wb").write(piped)
        parts.append(exact_fixture("WAVPIPE", p("pipe.wav"), 16))
        ffmpeg_make(p("s16.aiff"), TONE_NOISE % (44100, "0.02"), "-c:a", "pcm_s16be")
        parts.append(exact_fixture("AIFF16", p("s16.aiff"), 16))

        # 12-bit WAV and AIFF: the samples sit left-justified in 16-bit containers,
        # which ffmpeg decodes and shravan returned as nothing before 2.9.3. Made by
        # declaring 12 bits in 16-bit files and clearing the four unused low bits.
        def depth12(src, dst, big):
            d = bytearray(open(src, "rb").read())
            fmt_id, data_id, order = (b"COMM", b"SSND", ">") if big else (b"fmt ", b"data", "<")
            pos = 12
            while pos + 8 <= len(d):
                cid = bytes(d[pos:pos + 4])
                size = struct.unpack(order + "I", d[pos + 4:pos + 8])[0]
                if cid == fmt_id:
                    off = pos + (14 if big else 22)
                    d[off:off + 2] = struct.pack(order + "H", 12)
                if cid == data_id:
                    start = pos + 8
                    if big:
                        start += 8 + struct.unpack(">I", d[pos + 8:pos + 12])[0]
                    low = 1 if big else 0
                    for i in range(start, pos + 8 + size - 1, 2):
                        d[i + low] &= 0xF0
                    break
                pos += 8 + size + (size & 1)
            open(dst, "wb").write(d)

        ffmpeg_make(p("w16.wav"), TONE_NOISE % (16000, "0.02"), "-ac", "1", "-c:a", "pcm_s16le")
        depth12(p("w16.wav"), p("w12.wav"), False)
        parts.append(exact_fixture("WAV12", p("w12.wav"), 12))
        ffmpeg_make(p("a16.aiff"), TONE_NOISE % (16000, "0.02"), "-ac", "1", "-c:a", "pcm_s16be")
        depth12(p("a16.aiff"), p("a12.aiff"), True)
        parts.append(exact_fixture("AIFF12", p("a12.aiff"), 12))

        parts.append(id3_fixtures(tmp))

        # ALAC in MP4: adaptive LPC and stereo unmixing at 16 bits, plus the shifted
        # low bytes at 24.
        ffmpeg_make(p("alac16.m4a"), TONE_NOISE % (44100, "0.06"),
                    "-c:a", "alac", "-sample_fmt", "s16p")
        parts.append(exact_fixture("ALAC16", p("alac16.m4a"), 16))
        ffmpeg_make(p("alac24.m4a"), TONE_NOISE % (48000, "0.05"),
                    "-c:a", "alac", "-sample_fmt", "s32p")
        require(alac_shift_bits(p("alac24.m4a")) > 0, "ALAC shifted low bytes")
        parts.append(exact_fixture("ALAC24", p("alac24.m4a"), 24))

        # ALAC in QuickTime at 96 kHz: a version 2 sample entry with the config
        # nested in a wave box, and a rate that does not fit the 16.16 field.
        ffmpeg_make(p("alac96.mov"), TONE_NOISE % (96000, "0.03"),
                    "-c:a", "alac", "-sample_fmt", "s16p")
        data = open(p("alac96.mov"), "rb").read()
        at = data.find(b"stsd") + 12
        require(int.from_bytes(data[at + 16:at + 18], "big") == 2, "QuickTime v2 entry")
        require(b"wave" in data[at:at + 200], "QuickTime wave box")
        parts.append(exact_fixture("ALACMOV", p("alac96.mov"), 16))

        # AAC at 22.05 kHz, a band layout other than 44.1/48, with TNS and with
        # channels on independent windows. PNS stays off: its noise is defined only
        # by energy, so it can never match a reference sample for sample.
        ffmpeg_make(p("aac22.aac"), STEREO_TRANSIENT % (22050, 22050, 22050, "0.6"),
                    "-c:a", "aac", "-b:a", "64k", "-aac_pns", "0", "-f", "adts")
        frames, independent = adts_first_frames(p("aac22.aac"), 8)
        require(independent > 0, "AAC common_window = 0")
        parts.append(lossy_fixture("AAC22", p("aac22.aac"), 53))

        # AAC in MP4 at 96 kHz: the rate only exists in the esds, and 96 kHz has its
        # own band layout.
        ffmpeg_make(p("aac96.m4a"), STEREO_TRANSIENT % (96000, 96000, 96000, "0.08"),
                    "-c:a", "aac", "-b:a", "128k", "-aac_pns", "0")
        parts.append(lossy_fixture("AAC96", p("aac96.m4a"), 61))
        parts.append(mp4_audit_fixtures(p))

        # MP3 at 8 kHz: short blocks with the 72-line region boundary. And at 44.1:
        # count1 table B, which LAME picks for high-frequency bursts.
        ffmpeg_make(p("mp38.mp3"), STEREO_TRANSIENT % (8000, 8000, 8000, "0.9"),
                    "-ac", "1", "-c:a", "libmp3lame", "-b:a", "16k",
                    "-write_xing", "0", "-id3v2_version", "0")
        parts.append(lossy_fixture("MP38", p("mp38.mp3"), 41))
        ffmpeg_make(p("mp344.mp3"), STEREO_TRANSIENT % (44100, 44100, 44100, "0.3"),
                    "-ac", "1", "-c:a", "libmp3lame", "-b:a", "64k",
                    "-write_xing", "0", "-id3v2_version", "0")
        parts.append(lossy_fixture("MP344", p("mp344.mp3"), 97))

        # Opus: libopus's packets judged against libopus's own decode of them, so
        # container and pre-skip cannot hide anything. Each stream exercises a
        # path that decoded wrongly before 2.9.3.
        # Wideband SILK concealment differs in a deep-PLC libopus (DRED/OSCE), so the
        # references come from a default build: the system library if it is one, or
        # OPUS_PREFIX (scripts/interop/build-libopus.sh makes one).
        link = ["-lopus", "-lm"]
        if os.environ.get("OPUS_PREFIX"):
            pre = os.environ["OPUS_PREFIX"]
            link = ["-I" + os.path.join(pre, "include"), os.path.join(pre, "lib", "libopus.a"), "-lm"]
        run("cc", "-O2", "-o", p("opusref"), os.path.join(ROOT, "scripts", "interop", "opusref.c"),
            *link)
        caps = run(p("opusref"), "caps").stdout.decode()
        if "deep_plc 0" not in caps:
            sys.exit("gen-interop-fixtures: the Opus references need a default-build libopus, and"
                     " this one is deep-PLC (%s). Build one with scripts/interop/build-libopus.sh"
                     " <prefix> and set OPUS_PREFIX=<prefix>." % caps.strip())

        def mixes(cfg_ranges, stereo_mixed=False):
            def check(out):
                cfgs, stereo = configs_of(out)
                for lo, hi in cfg_ranges:
                    require(any(lo <= c <= hi for c in cfgs), "Opus configs %d-%d in %s" % (lo, hi, out))
                if stereo_mixed:
                    total = sum(cfgs.values())
                    require(0 < stereo < total, "Opus mono and stereo packets: " + out)
            return check

        # 2.5 ms CELT frames: the postfilter runs one segment and applies its new
        # parameters a frame late.
        text, _ = opus_fixture(tmp, "OPUS25", 28, 2, 30, 13, want=mixes([(28, 28)]))
        parts.append(text)
        # Inband FEC: LBRR frames precede the regular ones and must be read past.
        # 40 ms stereo SILK (joint LBRR flags, both channels) and mono hybrid.
        text, out = opus_fixture(tmp, "OPUSFEC", 9, 2, 24, 83, "dur=1920", "fec=30",
                                 want=mixes([(10, 10)]))
        require(any(silk_lbrr(q, 2, 2) for q in opus_packets(p("OPUSFEC.pk"))),
                "Opus FEC stream carries LBRR data")
        parts.append(text)
        text, _ = opus_fixture(tmp, "OPUSHFEC", 13, 1, 24, 47, "fec=20", want=mixes([(13, 13)]))
        require(any(silk_lbrr(q, 1, 1) for q in opus_packets(p("OPUSHFEC.pk"))),
                "Opus hybrid FEC stream carries LBRR data")
        parts.append(text)
        # Stereo SILK whose channels alternate between identical and independent:
        # mid-only frames, the side channel restarting, and in 60 ms packets the
        # side frame coded without the LTP scaling index.
        text, _ = opus_fixture(tmp, "OPUSMID", 9, 2, 20, 97, "dur=2880", "sig=1",
                               want=mixes([(11, 11)]))
        parts.append(text)
        # Mode switches (all six transitions, with their redundant CELT frames) and
        # mono/stereo packet switches in one stereo stream.
        text, _ = opus_fixture(tmp, "OPUSSW", 31, 2, 48, 107, "sw=3", "chsw=4",
                               want=mixes([(0, 11), (12, 15), (16, 31)], stereo_mixed=True))
        parts.append(text)
        # SILK bandwidth switches (the decoder retunes rather than restarts) and
        # 10/20 ms frame-length switches (it keeps its state).
        text, _ = opus_fixture(tmp, "OPUSBW", 1, 2, 60, 127, "bwsw=5", want=mixes([(1, 1), (5, 5)]))
        parts.append(text)
        text, _ = opus_fixture(tmp, "OPUSFS", 0, 2, 40, 59, "fsw=4", want=mixes([(0, 0), (1, 1)]))
        parts.append(text)
        # CELT bandwidth switches: bands outside the current [start, end) are
        # cleared, so the energy prediction restarts when a band comes back.
        text, _ = opus_fixture(tmp, "OPUSCBW", 31, 2, 40, 83, "bwsw=4",
                               want=mixes([(19, 19), (23, 23), (27, 27), (31, 31)]))
        parts.append(text)

        # Packet-loss concealment, which libopus runs for DTX frames and for a mode
        # switch whose redundant CELT frame was dropped (low bit rates): a 5 ms
        # concealment in the old mode is cross-faded into the new one. This stream
        # switches modes at 10 kb/s and hits both directions.
        text, _ = opus_fixture(tmp, "OPUSPLC", 31, 2, 36, 151, "sw=2", "br=10000", "dur=1920",
                               want=mixes([(10, 10), (15, 15), (31, 31)]))
        parts.append(text)
        # DTX: SILK concealment and comfort noise, then the first good frame faded in
        # from the concealment's energy and decoded with bandwidth-expanded LPC.
        text, out = opus_fixture(tmp, "OPUSDTX", 5, 2, 54, 113, "dtx=1", "sig=3", "voip=1",
                                 want=mixes([(5, 5)]))
        require("dtx" in out, "Opus DTX frames in the stream: " + out)
        parts.append(text)
        text, out = opus_fixture(tmp, "OPUSCDTX", 31, 1, 54, 59, "dtx=1", "sig=3",
                                 want=mixes([(31, 31)]))
        require("dtx" in out, "Opus CELT DTX frames in the stream: " + out)
        parts.append(text)

        # Lost packets (written as empty packets; libopus conceals them for the
        # previous packet's duration): after a loss SILK expands its LPC bandwidth,
        # resets the gain clamp and keeps a voiced tail, and CELT clamps its energy
        # prediction and folds the concealed overlap. 2.5 ms CELT, and mediumband
        # SILK (see the DTX note on 16 kHz).
        text, out = opus_fixture(tmp, "OPUSCLOSS", 28, 2, 60, 23, "lose=5", want=mixes([(28, 28)]))
        require("lost" in out, "Opus lost packets: " + out)
        parts.append(text)
        text, out = opus_fixture(tmp, "OPUSSLOSS", 5, 2, 60, 89, "lose=5", want=mixes([(5, 5)]))
        require("lost" in out, "Opus lost packets: " + out)
        parts.append(text)

        # The same at 16 kHz, wideband SILK and hybrid, where a deep-PLC libopus skips
        # the fade-in after concealment and a default build (these references) keeps
        # it. Until 2.9.3 this was checked only against the deep-PLC library.
        text, out = opus_fixture(tmp, "OPUSWBLOSS", 9, 1, 60, 47, "lose=6", want=mixes([(9, 9)]))
        require("lost" in out, "Opus WB lost packets: " + out)
        parts.append(text)
        text, out = opus_fixture(tmp, "OPUSHYBLOSS", 13, 2, 40, 71, "lose=3", want=mixes([(13, 13)]))
        require("lost" in out, "Opus hybrid lost packets: " + out)
        parts.append(text)
        text, out = opus_fixture(tmp, "OPUSWBDTX", 9, 2, 54, 113, "dtx=1", "sig=3", "voip=1",
                                 want=mixes([(9, 9)]))
        require("dtx" in out, "Opus WB DTX frames in the stream: " + out)
        parts.append(text)

        # Every CELT packet lost: the CELT->SILK redundant frames then follow
        # SILK-only frames, and libopus ignores their audio (the CELT decoder they
        # continue is stale). And losses across the silence gaps, where the gain
        # predictor's clamp must reset.
        text, out = opus_fixture(tmp, "OPUSC2S", 31, 2, 72, 211, "swmb=1", "lose=2",
                                 want=mixes([(5, 5)]))
        require("lost" in out, "Opus lost packets: " + out)
        parts.append(text)
        text, out = opus_fixture(tmp, "OPUSGCLAMP", 5, 2, 60, 89, "lose=5", "sig=3",
                                 want=mixes([(5, 5)]))
        parts.append(text)

        # MPEG audio conformance (ISO/IEC 11172-4, 13818-4): paths no available
        # encoder writes. Layer I and II joint stereo (the right channel reads its
        # scalefactors with the left channel's allocation above the bound); MPEG-2
        # intensity + M/S stereo with short blocks (the intensity channel's
        # scalefactor partition, illegal positions, and the reorder of the bands
        # intensity fills); MPEG-1 intensity stereo, mode by mode, with mono/stereo
        # switches; free-format Layer III.
        vec = minimp3_vectors(tmp)
        parts.append(iso_excerpt(vec, "ISOL1JS", "l1-fl1", 0, 6, 0, 13))
        parts.append(iso_excerpt(vec, "ISOL2JS", "l2-fl10", 0, 4, 0, 29))
        parts.append(iso_excerpt(vec, "ISOLSFIS", "l3-test46", 135, 41, 2, 47))
        parts.append(iso_excerpt(vec, "ISOMODE", "l3-he_mode", 0, 66, 0, 97))
        parts.append(iso_excerpt(vec, "ISOFREE", "l3-he_free", 0, 8, 0, 17))

        # A LAME file as written by default, with its Info frame: an index, not
        # audio. Judged against ffmpeg without gapless trimming (skip_manual), which
        # is what shravan does until encoder delay and padding are applied.
        ffmpeg_make(p("info.mp3"), TONE_NOISE % (44100, "0.3"), "-c:a", "libmp3lame", "-b:a", "64k")
        require(b"Info" in open(p("info.mp3"), "rb").read()[:4096], "LAME Info frame")
        parts.append(lossy_fixture("MP3INFO", p("info.mp3"), 37, extra=("-flags2", "+skip_manual")))

        # Ogg Opus with a non-zero header output gain, which RFC 7845 says a decoder
        # MUST apply. ffmpeg applies it; shravan ignored it before 2.9.3.
        ffmpeg_make(p("gain.opus"), TONE_NOISE % (48000, "0.3"), "-c:a", "libopus", "-b:a", "48k",
                    "-fflags", "+bitexact")
        data = bytearray(open(p("gain.opus"), "rb").read())
        body = 27 + data[26]
        require(data[body:body + 8] == b"OpusHead", "OpusHead in the first page")
        data[body + 16:body + 18] = (-6 * 256).to_bytes(2, "little", signed=True)
        page_len = body + sum(data[27:body])
        data[22:26] = b"\0\0\0\0"
        data[22:26] = ogg_crc(bytes(data[:page_len])).to_bytes(4, "little")
        open(p("gain.opus"), "wb").write(data)
        parts.append(lossy_fixture("OGGGAIN", p("gain.opus"), 61))

    with open(OUT, "w") as f:
        f.write("".join(parts))
    print("wrote %s (%d bytes)" % (OUT, os.path.getsize(OUT)))


if __name__ == "__main__":
    main()
