/* Golden-vector generator: runs real ISA-L (v2.32.1, `_base` reference
 * implementations, compiled from unmodified upstream sources) over
 * deterministic inputs and emits a binary vector file the Rust conformance
 * tests replay byte-for-byte.
 *
 * Build (any gcc, no autotools, no nasm):
 *   gcc -O2 -I <isal_headers_dir> gen_vectors.c <isal_src>/ec_base.c -o genvec
 *   ./genvec encode_vectors.bin
 *
 * File format (all integers little-endian, written on x86):
 *   magic "REV1", u32 case_count, then per case:
 *     u8 kind (0 = gf_gen_rs_matrix, 1 = gf_gen_cauchy1_matrix)
 *     u16 k, u16 p, u32 len
 *     k*len data bytes, p*k*32 gftbls bytes, p*len parity bytes
 *
 * Data is splitmix64 output seeded from (kind, k, p, len) as below, so the
 * file is regenerable bit-for-bit. The generator also asserts, in C, that a
 * full ec_encode_data_update_base sequence reproduces the one-shot
 * ec_encode_data_base output — so the update-equivalence contract our API
 * documents is checked against the reference itself before vectors ship. */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "erasure_code.h"

static uint64_t sm_state;

static uint64_t
sm_next(void)
{
        sm_state += 0x9E3779B97F4A7C15ULL;
        uint64_t z = sm_state;
        z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
        z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
        return z ^ (z >> 31);
}

struct cfg {
        int kind, k, p;
};

int
main(int argc, char **argv)
{
        if (argc != 2) {
                fprintf(stderr, "usage: %s <out.bin>\n", argv[0]);
                return 2;
        }
        FILE *f = fopen(argv[1], "wb");
        if (!f) {
                perror("fopen");
                return 1;
        }

        /* RS configs sit inside ISA-L's documented Vandermonde-safe region;
         * Cauchy covers the ERASCORP grid. */
        static const struct cfg cfgs[] = {
                { 0, 3, 8 },  { 0, 5, 5 },   { 0, 10, 4 }, { 0, 21, 4 }, { 0, 4, 21 },
                { 1, 4, 2 },  { 1, 8, 2 },   { 1, 10, 4 }, { 1, 16, 4 }, { 1, 20, 8 },
                { 1, 32, 8 },
        };
        static const int lens[] = { 1, 31, 32, 33, 96, 1024, 4113 };
        const int ncfg = sizeof(cfgs) / sizeof(cfgs[0]);
        const int nlen = sizeof(lens) / sizeof(lens[0]);

        fwrite("REV1", 1, 4, f);
        uint32_t count = (uint32_t) (ncfg * nlen);
        fwrite(&count, 4, 1, f);

        for (int ci = 0; ci < ncfg; ci++) {
                for (int li = 0; li < nlen; li++) {
                        const int kind = cfgs[ci].kind, k = cfgs[ci].k, p = cfgs[ci].p;
                        const int m = k + p, len = lens[li];

                        unsigned char *a = malloc((size_t) m * k);
                        unsigned char *g = malloc((size_t) p * k * 32);
                        unsigned char *data = malloc((size_t) k * len);
                        unsigned char *par = malloc((size_t) p * len);
                        unsigned char *par2 = calloc((size_t) p, (size_t) len);
                        unsigned char **dp = malloc(sizeof(*dp) * k);
                        unsigned char **pp = malloc(sizeof(*pp) * p);
                        unsigned char **pp2 = malloc(sizeof(*pp2) * p);
                        if (!a || !g || !data || !par || !par2 || !dp || !pp || !pp2) {
                                fprintf(stderr, "oom\n");
                                return 1;
                        }

                        if (kind == 0)
                                gf_gen_rs_matrix(a, m, k);
                        else
                                gf_gen_cauchy1_matrix(a, m, k);
                        ec_init_tables_base(k, p, &a[k * k], g);

                        sm_state = (uint64_t) kind * 1000003u + (uint64_t) k * 10007u +
                                   (uint64_t) p * 101u + (uint64_t) len;
                        for (int i = 0; i < k * len; i++)
                                data[i] = (unsigned char) sm_next();

                        for (int j = 0; j < k; j++)
                                dp[j] = &data[(size_t) j * len];
                        for (int l = 0; l < p; l++) {
                                pp[l] = &par[(size_t) l * len];
                                pp2[l] = &par2[(size_t) l * len];
                        }

                        ec_encode_data_base(len, k, p, g, dp, pp);

                        /* Reference-side check: update sequence == one-shot. */
                        for (int j = 0; j < k; j++)
                                ec_encode_data_update_base(len, k, p, j, g, dp[j], pp2);
                        if (memcmp(par, par2, (size_t) p * len) != 0) {
                                fprintf(stderr, "update != encode at kind=%d k=%d p=%d len=%d\n",
                                        kind, k, p, len);
                                return 1;
                        }

                        const uint8_t kind8 = (uint8_t) kind;
                        const uint16_t k16 = (uint16_t) k, p16 = (uint16_t) p;
                        const uint32_t len32 = (uint32_t) len;
                        fwrite(&kind8, 1, 1, f);
                        fwrite(&k16, 2, 1, f);
                        fwrite(&p16, 2, 1, f);
                        fwrite(&len32, 4, 1, f);
                        fwrite(data, 1, (size_t) k * len, f);
                        fwrite(g, 1, (size_t) p * k * 32, f);
                        fwrite(par, 1, (size_t) p * len, f);

                        free(a); free(g); free(data); free(par); free(par2);
                        free(dp); free(pp); free(pp2);
                }
        }

        long size = ftell(f);
        fclose(f);
        printf("%u cases, %ld bytes\n", count, size);
        return 0;
}
