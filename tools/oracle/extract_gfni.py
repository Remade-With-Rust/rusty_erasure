"""Extract ISA-L's gf_table_gfni (256 x u64 affine matrices for GF2P8AFFINEQB)
from ec_base.h as golden data: corpus/golden/gf_table_gfni.bin, little-endian
u64s. Same provenance discipline as extract_tables.py."""

import hashlib
import re
import struct
import sys
from pathlib import Path

SRC = Path(sys.argv[1])  # ec_base.h
OUT = Path(sys.argv[2])  # corpus/golden dir

text = SRC.read_text()
src_sha = hashlib.sha256(SRC.read_bytes()).hexdigest()

m = re.search(r"uint64_t gf_table_gfni\[256\]\s*=\s*\{(.*?)\};", text, re.S)
if not m:
    sys.exit("gf_table_gfni not found")
body = re.sub(r"//.*?$|/\*.*?\*/", "", m.group(1), flags=re.S | re.M)
vals = [int(tok, 0) for tok in re.findall(r"0[xX][0-9a-fA-F]+|\d+", body)]
if len(vals) != 256:
    sys.exit(f"expected 256 entries, parsed {len(vals)}")
blob = b"".join(struct.pack("<Q", v) for v in vals)
out = OUT / "gf_table_gfni.bin"
out.write_bytes(blob)
print(f"gf_table_gfni: 256 entries -> {out.name}")
print(f"source sha256 {src_sha}")
print(f"output sha256 {hashlib.sha256(blob).hexdigest()}")
