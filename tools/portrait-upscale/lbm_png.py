#!/usr/bin/env python3
"""LBM (FORM PBM) <-> PNG, with no third-party imaging library available.

Decoding and encoding both live here so the palette can be carried across unchanged: the safest
requantisation target for a portrait is the 256 colours that portrait ALREADY uses, because those
are known to display correctly in the engine today. Whether a NEW palette would be honoured or
remapped onto a fixed screen palette is not established, so it is not assumed here.
"""
import struct, zlib, pathlib

def chunks(d):
    off = 12
    while off + 8 <= len(d):
        cid = d[off:off+4]; sz = struct.unpack(">I", d[off+4:off+8])[0]
        yield cid, d[off+8:off+8+sz]
        off += 8 + sz + (sz & 1)

def decode(path):
    d = pathlib.Path(path).read_bytes()
    assert d[:4] == b"FORM" and d[8:12] == b"PBM ", "not a FORM PBM"
    bm = cm = bd = None
    for cid, body in chunks(d):
        if cid == b"BMHD": bm = body
        elif cid == b"CMAP": cm = body
        elif cid == b"BODY": bd = body
    w, h = struct.unpack(">HH", bm[:4]); comp = bm[10]
    if comp == 1:
        px = bytearray(); i = 0
        while i < len(bd) and len(px) < w*h:
            n = bd[i]; i += 1
            if n < 128: px += bd[i:i+n+1]; i += n+1
            elif n > 128: px += bytes([bd[i]]) * (257-n); i += 1
    else:
        px = bytearray(bd[:w*h])
    return w, h, bytes(px), [tuple(cm[i*3:i*3+3]) for i in range(len(cm)//3)], bm

def write_png(path, w, h, rgb_rows, scale=1):
    raw = b"".join(b"\x00" + b"".join(bytes(p)*scale for p in row) for row in rgb_rows for _ in range(scale))
    def ck(t, d):
        c = t + d
        return struct.pack(">I", len(d)) + c + struct.pack(">I", zlib.crc32(c) & 0xffffffff)
    hdr = struct.pack(">IIBBBBB", w*scale, h*scale, 8, 2, 0, 0, 0)
    pathlib.Path(path).write_bytes(
        b"\x89PNG\r\n\x1a\n" + ck(b"IHDR", hdr) + ck(b"IDAT", zlib.compress(raw, 9)) + ck(b"IEND", b""))

def read_png_rgb(path):
    """Minimal PNG reader: 8-bit truecolour or truecolour+alpha, non-interlaced."""
    d = pathlib.Path(path).read_bytes()
    assert d[:8] == b"\x89PNG\r\n\x1a\n"
    off = 8; idat = b""; w = h = bd = ct = None
    while off < len(d):
        sz = struct.unpack(">I", d[off:off+4])[0]; typ = d[off+4:off+8]
        body = d[off+8:off+8+sz]
        if typ == b"IHDR":
            w, h, bd, ct = struct.unpack(">IIBB", body[:10])
        elif typ == b"IDAT": idat += body
        elif typ == b"IEND": break
        off += 12 + sz
    assert bd == 8 and ct in (2, 6), f"unsupported PNG: depth={bd} colour={ct}"
    nch = 3 if ct == 2 else 4
    raw = zlib.decompress(idat); stride = w * nch
    out = []; prev = bytearray(stride)
    pos = 0
    for _ in range(h):
        f = raw[pos]; pos += 1
        line = bytearray(raw[pos:pos+stride]); pos += stride
        for i in range(stride):
            a = line[i-nch] if i >= nch else 0
            b = prev[i]
            c = prev[i-nch] if i >= nch else 0
            if f == 1: line[i] = (line[i] + a) & 255
            elif f == 2: line[i] = (line[i] + b) & 255
            elif f == 3: line[i] = (line[i] + (a+b)//2) & 255
            elif f == 4:
                p = a + b - c; pa, pb, pc = abs(p-a), abs(p-b), abs(p-c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 255
        out.append([tuple(line[x*nch:x*nch+3]) for x in range(w)])
        prev = line
    return w, h, out

def byterun1(row):
    out = bytearray(); i = 0; n = len(row)
    while i < n:
        run = 1
        while i+run < n and row[i+run] == row[i] and run < 128: run += 1
        if run >= 2:
            out.append(257-run); out.append(row[i]); i += run
        else:
            lit = bytearray()
            while i < n and len(lit) < 128:
                if i+2 < n and row[i] == row[i+1] == row[i+2]: break
                lit.append(row[i]); i += 1
            out.append(len(lit)-1); out.extend(lit)
    return bytes(out)

def encode(dest, w, h, indices, palette, template_bmhd):
    bm = bytearray(template_bmhd)
    struct.pack_into(">HH", bm, 0, w, h)
    struct.pack_into(">hh", bm, 16, w, h)
    bm[10] = 1
    body = bytearray()
    for y in range(h): body += byterun1(indices[y*w:(y+1)*w])
    cm = b"".join(bytes(c) for c in palette)
    out = bytearray()
    for cid, data in ((b"BMHD", bytes(bm)), (b"CMAP", cm), (b"BODY", bytes(body))):
        out += cid + struct.pack(">I", len(data)) + data
        if len(data) & 1: out += b"\x00"
    pathlib.Path(dest).write_bytes(b"FORM" + struct.pack(">I", 4+len(out)) + b"PBM " + bytes(out))
    return len(out) + 12
