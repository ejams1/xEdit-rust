"""Writes a plugin with GMST float records whose values probe the oracle's rounding."""
import random
import struct
import sys

out = sys.argv[1]
random.seed(1)
values = []
# Exact ties at the seventh decimal: multiples of 1/128 and 1/1024 etc. with odd numerators.
for k in range(300):
    base = random.choice([100, 1000, 3000, 9000, 14000, 60000, 100000])
    frac = random.choice([1, 3, 5, 7, 9, 11, 13, 15]) / random.choice([16, 32, 64, 128, 256, 512, 1024])
    values.append(base + random.randint(0, 999) + frac)
# Random floats of various magnitudes.
for k in range(1500):
    mag = random.choice([1, 10, 100, 1000, 10000, 100000])
    values.append(random.uniform(-mag, mag))
# Values with the seventh decimal 5..9 (would round up) and 0..4.
f32s = []
for v in values:
    f = struct.unpack('<f', struct.pack('<f', v))[0]
    f32s.append(f)

records = b''
for i, f in enumerate(f32s):
    edid = ('fProbe%04d' % i).encode() + b'\0'
    data = struct.pack('<f', f)
    body = b'EDID' + struct.pack('<H', len(edid)) + edid + b'DATA' + struct.pack('<H', 4) + data
    header = b'GMST' + struct.pack('<IIIIHH', len(body), 0, 0x00000800 + i, 0, 44, 0)
    records += header + body
group = b'GRUP' + struct.pack('<I', 24 + len(records)) + b'GMST' + struct.pack('<iII', 0, 0, 0)
hedr = b'HEDR' + struct.pack('<H', 12) + struct.pack('<fII', 1.7, len(f32s), 0x800 + len(f32s))
cnam = b'CNAM' + struct.pack('<H', 6) + b'probe\0'
tes4_body = hedr + cnam
tes4 = b'TES4' + struct.pack('<IIIIHH', len(tes4_body), 0, 0, 0, 44, 0) + tes4_body
open(out, 'wb').write(tes4 + group + records)
with open(out + '.values.txt', 'w') as fh:
    for i, f in enumerate(f32s):
        fh.write('%d %r %08X\n' % (i, f, struct.unpack('<I', struct.pack('<f', f))[0]))
print(len(f32s), 'records')
