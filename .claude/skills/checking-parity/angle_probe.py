"""Pair the oracle's 4-digit REFR rotation strings with the raw radians.

Usage: angle_probe.py <plugin> <oracle dump> [pairs.pkl]
"""
import re, struct, zlib, sys, math

data = open(sys.argv[1], 'rb').read()

# raw rotations by formid for REFR records (DATA = 6 floats)
raw = {}
pos = 0
while True:
    pos = data.find(b'REFR', pos)
    if pos < 0:
        break
    size, flags, fid = struct.unpack_from('<III', data, pos + 4)
    if size < 0x100000 and pos + 24 + size <= len(data):
        body = data[pos + 24:pos + 24 + size]
        ok = True
        if flags & 0x40000:
            try:
                body = zlib.decompress(body[4:])
            except Exception:
                ok = False
        if ok:
            p = 0
            while p + 6 <= len(body):
                s = body[p:p + 4]; l = struct.unpack_from('<H', body, p + 4)[0]; p += 6
                if s == b'DATA' and l == 24:
                    raw[fid] = struct.unpack_from('<6f', body, p)
                    break
                if s == b'XXXX':
                    break
                p += l
    pos += 4
print('refr raw', len(raw), file=sys.stderr)

# oracle rotation strings per record
oracle = {}
fid = None
rot_lines = []
rx = re.compile(rb'^\s+([XYZ]): (-?\d+\.\d{4})$')
for line in open(sys.argv[2], 'rb'):
    line = line.rstrip(b'\r\n')
    m = re.match(rb'^\s+FormID: REFR - [^\[]*\[([0-9A-F]{8})\]', line)
    if m:
        fid = int(m.group(1), 16); rot_lines = []; in_rot = False
        continue
    if fid is None:
        continue
    if b'Rotation [S]: Rot(' in line and fid not in oracle:
        in_rot = True; rot_lines = []
        continue
    if in_rot:
        m = rx.match(line)
        if m:
            rot_lines.append(m.group(2).decode())
            if len(rot_lines) == 3:
                oracle[fid] = rot_lines; in_rot = False
        else:
            in_rot = False
print('oracle rots', len(oracle), file=sys.stderr)

SCALE = 180 / math.pi
TWO_PI = 2 * math.pi

def normalize(v):
    while v < 0:
        v += TWO_PI
    while v > TWO_PI:
        v -= TWO_PI
    return v

def fixed4(v):
    f = 0.0001
    r = round(v / f) * f
    return f'{r:.4f}'

pairs = []
for fid, strs in oracle.items():
    if fid not in raw:
        continue
    r = raw[fid][3:6]
    for k in range(3):
        v = normalize(r[k]) * SCALE
        pairs.append((fid, k, r[k], v, strs[k]))

mism = [(fid, k, rr, v, s) for fid, k, rr, v, s in pairs if fixed4(v) != s]
print('pairs', len(pairs), 'mismatch under port model', len(mism))
for fid, k, rr, v, s in mism[:40]:
    print(f'{fid:08X} {k} raw={rr!r} deg={v!r} oracle={s} port={fixed4(v)} rem={(v/0.0001)%1:.4f}')
rems = [((v / 0.0001) % 1) for fid, k, rr, v, s in mism]
print('remainder range', min(rems), max(rems))
if len(sys.argv) > 3:
    import pickle
    pickle.dump(pairs, open(sys.argv[3], 'wb'))
