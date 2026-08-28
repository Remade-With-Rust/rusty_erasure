/* ISA-L RAID perf arm (rig-only): dispatched xor_gen / pq_gen on process CPU
 * time, SOURCE-BYTES basis — mirrors `rerasure bench --op xor|pq` exactly.
 * Build: gcc -O2 -I ~/isa-l/include raid_arm.c ~/isa-l/.libs/libisal.a -o raid_arm
 * Run:   taskset -c 2 ./raid_arm <nsrc> <len> <reps> <op: 0=xor 1=pq> */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "raid.h"

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
        if (argc != 5)
                return 2;
        const int nsrc = atoi(argv[1]), len = atoi(argv[2]);
        const long reps = atol(argv[3]);
        const int op = atoi(argv[4]);

        void **arr = malloc(sizeof(void *) * (nsrc + 2));
        uint64_t st = ((uint64_t) nsrc << 32) | (uint64_t) len;
        for (int j = 0; j < nsrc + 2; j++) {
                if (posix_memalign(&arr[j], 32, (size_t) len))
                        return 1;
                unsigned char *b = arr[j];
                for (int i = 0; i < len; i++) {
                        st += 0x9E3779B97F4A7C15ULL;
                        uint64_t z = st;
                        z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
                        z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
                        b[i] = (unsigned char) (z ^ (z >> 31));
                }
        }
        for (int w = 0; w < 3; w++)
                op ? pq_gen(nsrc + 2, len, arr) : xor_gen(nsrc + 1, len, arr);
        const double t0 = cpu_now();
        for (long r = 0; r < reps; r++)
                op ? pq_gen(nsrc + 2, len, arr) : xor_gen(nsrc + 1, len, arr);
        const double dt = cpu_now() - t0;
        printf("arm=isal_%s nsrc=%d len=%d reps=%ld cpu_s=%.3f src_GBps=%.3f\n",
               op ? "pq" : "xor", nsrc, len, reps, dt,
               (double) nsrc * len * reps / dt / 1e9);
        return 0;
}
