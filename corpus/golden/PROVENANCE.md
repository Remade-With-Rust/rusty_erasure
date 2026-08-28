# Golden GF(2^8) tables � provenance

Extracted as **data** from Intel ISA-L `erasure_code/ec_base.h` at tag **v2.32.1**
(https://raw.githubusercontent.com/intel/isa-l/v2.32.1/erasure_code/ec_base.h),
source sha256 `b9b3d8e58e77be5642c28bebb2f9a400d03aeb34508c5cb34549289af329d593`, by `tools/oracle/extract_tables.py`.
No ISA-L source text is included in this repository (mission plan �9.2).

| file | elements | sha256 |
|---|---|---|
| `encode_vectors.bin` | 77 cases (11 configs × 7 lengths; format doc in `tools/oracle/gen_vectors.c`) | `c408ebdff37349416d88dec6197472492e7b8269522a624c8e8ec3a6b59d8c26` |
| `gff_base.bin` | 256 | `8262c77b024996fe63bfa734855c87410a2e5a9b18b019693896f60f23838142` |
| `gflog_base.bin` | 256 | `7c551932953617708efec9497d4dc112b413225b1d099b8446b0fd4759979746` |
| `gf_mul_table_base.bin` | 65536 | `003d1a609783d2740b9b3f00b0cd9e43e42c4f3eedc5ff54ec1709996d52e1e0` |
| `gf_inv_table_base.bin` | 256 | `ce85f43612c0a6d03939cc3dfe9ca877032d017fb26aca602b696b74e5600d72` |
