/* FULL-GRID conformance generator (rig-only, output NOT checked in): every
 * Cauchy config k=1..32 x p=1..8 plus every Vandermonde-safe-region config in
 * that range, through real ISA-L _base code, in the same REV1 format as
 * encode_vectors.bin. The Rust side replays it via the env-gated fullgrid
 * test. Run per benchmark campaign; the ledger records the verdict.
 *
 * Build: gcc -O2 -I ~/isa-l/include gen_fullgrid.c ~/isa-l/erasure_code/ec_base.c -o genfull */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "erasure_code.h"

static uint64_t st;
static uint64_t
nx(void)
{
        st += 0x9E3779B97F4A7C15ULL;
        uint64_t z = st;
        z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
        z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
        return z ^ (z >> 31);
}

static int
vand_safe(int k, int m)
{
        return k <= 3 || (k == 4 && m <= 25) || (k == 5 && m <= 10) ||
               (k <= 21 && m - k == 4) || m - k <= 3;
}

static void
emit(FILE *f, int kind, int k, int p, int len)
{
        const int m = k + p;
        unsigned char *a = malloc((size_t) m * k);
        unsigned char *g = malloc((size_t) p * k * 32);
        unsigned char *data = malloc((size_t) k * len);
        unsigned char *par = malloc((size_t) p * len);
        unsigned char **dp = malloc(sizeof(*dp) * k);
        unsigned char **pp = malloc(sizeof(*pp) * p);
        if (kind == 0)
                gf_gen_rs_matrix(a, m, k);
        else
                gf_gen_cauchy1_matrix(a, m, k);
        ec_init_tables_base(k, p, &a[k * k], g);
        st = (uint64_t) kind * 1000003u + (uint64_t) k * 10007u + (uint64_t) p * 101u +
             (uint64_t) len;
        for (int i = 0; i < k * len; i++)
                data[i] = (unsigned char) nx();
        for (int j = 0; j < k; j++)
                dp[j] = &data[(size_t) j * len];
        for (int l = 0; l < p; l++)
                pp[l] = &par[(size_t) l * len];
        ec_encode_data_base(len, k, p, g, dp, pp);

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
        free(a); free(g); free(data); free(par); free(dp); free(pp);
}

int
main(int argc, char **argv)
{
        if (argc != 2)
                return 2;
        FILE *f = fopen(argv[1], "wb");
        if (!f)
                return 1;
        static const int lens[] = { 1, 33, 1024 };

        uint32_t count = 0;
        for (int k = 1; k <= 32; k++)
                for (int p = 1; p <= 8; p++) {
                        count += 3;                    /* cauchy at 3 lens  */
                        if (vand_safe(k, k + p))
                                count += 1;            /* rs at len 1024    */
                }
        fwrite("REV1", 1, 4, f);
        fwrite(&count, 4, 1, f);
        for (int k = 1; k <= 32; k++)
                for (int p = 1; p <= 8; p++) {
                        for (int li = 0; li < 3; li++)
                                emit(f, 1, k, p, lens[li]);
                        if (vand_safe(k, k + p))
                                emit(f, 0, k, p, 1024);
                }
        long size = ftell(f);
        fclose(f);
        printf("%u cases, %ld bytes\n", count, size);
        return 0;
}
