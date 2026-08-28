/* RAID golden-vector generator (rig-only). Two authorities, one file:
 *  kind 0: real dispatched ISA-L xor_gen/pq_gen (SIMD build, 32-aligned
 *          buffers, len % 32 == 0 per their contract);
 *  kind 1: ISA-L's own byte-wise reference math (the exact expressions of
 *          pq_check_base/xor_gen_base) for arbitrary/odd lengths, which the
 *          dispatched kernels do not accept.
 *
 * Build:  gcc -O2 -I ~/isa-l/include gen_raid_vectors.c ~/isa-l/.libs/libisal.a -o genraid
 * Format: "RRV1", u32 count, per case: u8 kind, u16 nsrc, u32 len,
 *         nsrc*len sources, len xor, len p, len q. Data: splitmix64 seeded
 *         from (kind, nsrc, len). */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "raid.h"

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

int
main(int argc, char **argv)
{
        if (argc != 2)
                return 2;
        FILE *f = fopen(argv[1], "wb");
        if (!f)
                return 1;

        static const int nsrcs[] = { 2, 4, 8, 15 };
        static const int alens[] = { 32, 64, 992, 4096 };
        static const int olens[] = { 1, 7, 13, 100, 1017 };
        const int NN = 4, NA = 4, NO = 5;

        fwrite("RRV1", 1, 4, f);
        uint32_t count = (uint32_t) (NN * (NA + NO));
        fwrite(&count, 4, 1, f);

        for (int ni = 0; ni < NN; ni++) {
                for (int li = 0; li < NA + NO; li++) {
                        const int kind = li < NA ? 0 : 1;
                        const int nsrc = nsrcs[ni];
                        const int len = kind == 0 ? alens[li] : olens[li - NA];

                        unsigned char **bufs = malloc(sizeof(*bufs) * (nsrc + 3));
                        for (int j = 0; j < nsrc + 3; j++) {
                                if (posix_memalign((void **) &bufs[j], 32, (size_t) len))
                                        return 1;
                                memset(bufs[j], 0, (size_t) len);
                        }
                        st = (uint64_t) kind * 1000003u + (uint64_t) nsrc * 10007u +
                             (uint64_t) len;
                        for (int j = 0; j < nsrc; j++)
                                for (int i = 0; i < len; i++)
                                        bufs[j][i] = (unsigned char) nx();
                        unsigned char *xorp = bufs[nsrc], *p = bufs[nsrc + 1],
                                      *q = bufs[nsrc + 2];

                        if (kind == 0) {
                                /* dispatched arms; array layouts per raid.h */
                                void **xa = malloc(sizeof(void *) * (nsrc + 1));
                                for (int j = 0; j < nsrc; j++)
                                        xa[j] = bufs[j];
                                xa[nsrc] = xorp;
                                if (xor_gen(nsrc + 1, len, xa))
                                        return 3;
                                void **pa = malloc(sizeof(void *) * (nsrc + 2));
                                for (int j = 0; j < nsrc; j++)
                                        pa[j] = bufs[j];
                                pa[nsrc] = p;
                                pa[nsrc + 1] = q;
                                if (pq_gen(nsrc + 2, len, pa))
                                        return 4;
                                free(xa);
                                free(pa);
                        } else {
                                /* byte-wise reference: the exact expressions of
                                 * xor_gen_base / pq_check_base */
                                for (int i = 0; i < len; i++) {
                                        unsigned char par = bufs[0][i];
                                        for (int j = 1; j < nsrc; j++)
                                                par ^= bufs[j][i];
                                        xorp[i] = par;
                                        unsigned char qb, pb, s;
                                        qb = pb = bufs[nsrc - 1][i];
                                        for (int j = nsrc - 2; j >= 0; j--) {
                                                s = bufs[j][i];
                                                pb ^= s;
                                                qb = s ^ ((unsigned char) (qb << 1) ^
                                                          ((qb & 0x80) ? 0x1d : 0));
                                        }
                                        p[i] = pb;
                                        q[i] = qb;
                                }
                        }

                        const uint8_t kind8 = (uint8_t) kind;
                        const uint16_t n16 = (uint16_t) nsrc;
                        const uint32_t len32 = (uint32_t) len;
                        fwrite(&kind8, 1, 1, f);
                        fwrite(&n16, 2, 1, f);
                        fwrite(&len32, 4, 1, f);
                        for (int j = 0; j < nsrc; j++)
                                fwrite(bufs[j], 1, (size_t) len, f);
                        fwrite(xorp, 1, (size_t) len, f);
                        fwrite(p, 1, (size_t) len, f);
                        fwrite(q, 1, (size_t) len, f);
                        for (int j = 0; j < nsrc + 3; j++)
                                free(bufs[j]);
                        free(bufs);
                }
        }
        long size = ftell(f);
        fclose(f);
        printf("%u cases, %ld bytes\n", count, size);
        return 0;
}
