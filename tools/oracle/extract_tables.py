"""Extract ISA-L's GF(2^8) base tables from ec_base.h (v2.32.1) as binary golden data.

Reads the C header from the scratchpad, parses the four static arrays, verifies
their element counts against the C declarations' semantics, and writes raw
binary files + a provenance manifest into the repo's corpus/golden/ directory.
The header itself never enters the repo; these outputs are data.
"""

import hashlib
import re
import sys
from pathlib import Path

SRC = Path(sys.argv[1])  # ec_base.h
OUT = Path(sys.argv[2])  # corpus/golden dir
OUT.mkdir(parents=True, exist_ok=True)

text = SRC.read_text()
src_sha = hashlib.sha256(SRC.read_bytes()).hexdigest()

EXPECT = {
    "gff_base": 256,           # exp table: gff_base[i] = 2^i in GF(2^8), poly 0x11d
    "gflog_base": 256,         # log table (entry 0 unused)
    "gf_mul_table_base": 65536,  # full product table, index b*256 + a
    "gf_inv_table_base": 256,  # inverse table (inv(0) defined as 0 by ISA-L)
}

manifest = [
    "# Golden GF(2^8) tables — provenance",
    "",
    "Extracted as **data** from Intel ISA-L `erasure_code/ec_base.h` at tag **v2.32.1**",
    "(https://raw.githubusercontent.com/intel/isa-l/v2.32.1/erasure_code/ec_base.h),",
    f"source sha256 `{src_sha}`, by `tools/oracle/extract_tables.py`.",
    "No ISA-L source text is included in this repository (mission plan §9.2).",
    "",
    "| file | elements | sha256 |",
    "|---|---|---|",
]

for name, expect_n in EXPECT.items():
    m = re.search(rf"unsigned char {name}\[\]\s*=\s*\{{(.*?)\}};", text, re.S)
    if not m:
        sys.exit(f"array {name} not found")
    body = re.sub(r"//.*?$|/\*.*?\*/", "", m.group(1), flags=re.S | re.M)
    vals = [int(tok, 0) for tok in re.findall(r"0[xX][0-9a-fA-F]+|\d+", body)]
    if len(vals) != expect_n:
        sys.exit(f"{name}: expected {expect_n} elements, parsed {len(vals)}")
    if not all(0 <= v <= 255 for v in vals):
        sys.exit(f"{name}: value out of byte range")
    blob = bytes(vals)
    out = OUT / f"{name}.bin"
    out.write_bytes(blob)
    manifest.append(f"| `{name}.bin` | {len(vals)} | `{hashlib.sha256(blob).hexdigest()}` |")
    print(f"{name}: {len(vals)} elements -> {out.name}")

# Cross-consistency of the extraction itself: the mul table must agree with
# the log/exp construction, and the inv table with exp(255 - log(a)).
gff = (OUT / "gff_base.bin").read_bytes()
glog = (OUT / "gflog_base.bin").read_bytes()
gmul = (OUT / "gf_mul_table_base.bin").read_bytes()
ginv = (OUT / "gf_inv_table_base.bin").read_bytes()

for a in range(256):
    for b in range(256):
        if a == 0 or b == 0:
            want = 0
        else:
            s = glog[a] + glog[b]
            want = gff[s - 255 if s > 254 else s]
        if gmul[b * 256 + a] != want:
            sys.exit(f"extraction inconsistent: mul[{a},{b}]")
for a in range(256):
    want = 0 if a == 0 else gff[255 - glog[a]]
    if ginv[a] != want:
        sys.exit(f"extraction inconsistent: inv[{a}]")
print("cross-consistency: mul and inv tables agree with log/exp construction")

(OUT / "PROVENANCE.md").write_text("\n".join(manifest) + "\n")
print("wrote PROVENANCE.md")
