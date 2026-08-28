/* Matched-ISA and dispatched perf arms for ISA-L (rig-only, nothing ships).
 *
 * Times ec_encode_data (runtime-dispatched — avx2_gfni on GFNI silicon) or
 * ec_encode_data_avx2 (the plain-AVX2 symbol, the matched-ISA arm for M4's
 * calibration bar), on PROCESS CPU time and a SOURCE-BYTES basis — the exact
 * method and basis of `rerasure bench` + tools/bench/ab_encode.ps1, so the
 * arms compare like for like. Prints a parity checksum for work identity.
 *
 * Build:  gcc -O2 -I ~/isa-l/include perf_arm.c ~/isa-l/.libs/libisal.a -o perf_arm
 * Run:    taskset -c 2 ./perf_arm <k> <p> <len> <reps> <arm: 0=dispatched 1=avx2>
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#include "erasure_code.h"

/* The per-ISA symbol is exported (deprecated) — declare it directly. */
extern void
ec_encode_data_avx2(int len, int k, int rows, unsigned char *gftbls, unsigned char **data,
                    unsigned char **coding);

static double
cpu_now(void)
{
        struct timespec ts;
        clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &ts);
        return (double) ts.tv_sec + (double) ts.tv_nsec / 1e9;
}

int
main(int argc, char **argv)
{
        if (argc != 6) {
                fprintf(stderr, "usage: %s <k> <p> <len> <reps> <arm 0|1>\n", argv[0]);
                return 2;
        }
        const int k = atoi(argv[1]), p = atoi(argv[2]), len = atoi(argv[3]);
        const long reps = atol(argv[4]);
        const int use_avx2 = atoi(argv[5]);
        const int m = k + p;

        unsigned char *a = malloc((size_t) m * k);
        unsigned char *g = malloc((size_t) p * k * 32);
        unsigned char **data = malloc(sizeof(*data) * k);
        unsigned char **coding = malloc(sizeof(*coding) * p);
        if (!a || !g || !data || !coding)
                return 1;
        gf_gen_cauchy1_matrix(a, m, k);
        /* The avx2 arm consumes NIBBLE-format tables; on GFNI silicon the
         * dispatched ec_init_tables emits AFFINE-format tables (found the
         * hard way: mixed formats produced wrong parity — checksums caught
         * it). Each arm gets the init its encode consumes. */
        if (use_avx2)
                ec_init_tables_base(k, p, &a[k * k], g);
        else
                ec_init_tables(k, p, &a[k * k], g);

        /* splitmix64 data, same seeding shape as rerasure bench. */
        uint64_t st = ((uint64_t) k << 32) | (uint64_t) len;
        for (int j = 0; j < k; j++) {
                data[j] = malloc((size_t) len);
                for (int i = 0; i < len; i++) {
                        st += 0x9E3779B97F4A7C15ULL;
                        uint64_t z = st;
                        z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
                        z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
                        data[j][i] = (unsigned char) (z ^ (z >> 31));
                }
        }
        for (int l = 0; l < p; l++)
                coding[l] = malloc((size_t) len);

        for (int w = 0; w < 3; w++) /* warmup */
                use_avx2 ? ec_encode_data_avx2(len, k, p, g, data, coding)
                         : ec_encode_data(len, k, p, g, data, coding);

        const double t0 = cpu_now();
        for (long r = 0; r < reps; r++)
                use_avx2 ? ec_encode_data_avx2(len, k, p, g, data, coding)
                         : ec_encode_data(len, k, p, g, data, coding);
        const double dt = cpu_now() - t0;

        unsigned char chk = 0;
        for (int l = 0; l < p; l++)
                for (int i = 0; i < len; i++)
                        chk ^= coding[l][i];

        printf("arm=%s k=%d p=%d len=%d reps=%ld cpu_s=%.3f src_GBps=%.3f checksum=0x%02x\n",
               use_avx2 ? "isal_avx2" : "isal_dispatched", k, p, len, reps, dt,
               (double) k * len * reps / dt / 1e9, chk);
        return 0;
}
