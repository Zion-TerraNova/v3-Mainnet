/*
 * ffi_wrapper_v3.cpp — extern "C" entry points for the Rust FFI layer.
 *
 * Bridges the C++ VerusHash API to the V3 native-ffi ABI:
 *   void    verushash_init(void)
 *   void    verushash_hash(header, header_len, nonce, output)
 *   int32_t verushash_verify(header, header_len, nonce, target)
 *   double  verushash_benchmark(iterations)
 *   const char* verushash_version(void)
 *
 * Copyright (c) 2024-2026 Zion Project — MIT License
 */

#include "compat.h"
#include "verus_hash.h"

#include <cstring>
#include <cstdint>
#include <cstdio>
#include <ctime>

extern "C" {

/* One-time global initialization. */
void verushash_init(void) {
    CVerusHash::init();
    CVerusHashV2::init();
}

/*
 * Compute VerusHash v2.2 of data[0..len].
 * Writes exactly 32 bytes to output.
 *
 * Uses CVerusHashV2bWriter semantics (Write + Finalize2b),
 * which includes verusclhash + keyed Haraka-512 finalization.
 * This matches hash2b2() in node-verushash (the PoW hash).
 */
void verushash_hash(
    const uint8_t* header,
    size_t         header_len,
    uint64_t       nonce,
    uint8_t*       output)
{
    /* Build the full buffer: header || nonce_LE8
     * VerusHash v2.2 hashes the entire block header (including nonce).
     * The nonce is appended as 8 little-endian bytes. */
    size_t buf_len = header_len + 8;
    uint8_t* buf = (uint8_t*)malloc(buf_len);
    if (!buf) {
        /* OOM — zero output and return */
        memset(output, 0, 32);
        return;
    }
    memcpy(buf, header, header_len);
    for (int i = 0; i < 8; i++) {
        buf[header_len + i] = (uint8_t)(nonce >> (8 * i));
    }

    /* CVerusHashV2 with solutionVersion = SOLUTION_VERUSHHASH_V2_2 (=4)
     * gives us the correct verusclhash_sv2_2 variant in Finalize2b. */
    CVerusHashV2 hasher(SOLUTION_VERUSHHASH_V2_2);
    hasher.Reset();
    hasher.Write(buf, buf_len);
    hasher.Finalize2b(output);

    free(buf);
}

/* Return 1 if hash <= target (big-endian comparison), 0 otherwise. */
int32_t verushash_verify(
    const uint8_t* header,
    size_t         header_len,
    uint64_t       nonce,
    const uint8_t* target)
{
    uint8_t hash[32];
    verushash_hash(header, header_len, nonce, hash);
    /* Big-endian comparison: hash <= target */
    for (int i = 0; i < 32; i++) {
        if (hash[i] < target[i]) return 1;
        if (hash[i] > target[i]) return 0;
    }
    return 1; /* equal */
}

/* Benchmark: returns hashes/sec */
double verushash_benchmark(int32_t iterations) {
    if (iterations <= 0) iterations = 1000;
    uint8_t header[76];
    memset(header, 0x56, 76);
    uint8_t out[32];
    struct timespec t0, t1;
    timespec_get(&t0, TIME_UTC);
    for (int32_t i = 0; i < iterations; i++) {
        header[0] = (uint8_t)(i & 0xFF);
        verushash_hash(header, 76, (uint64_t)i, out);
    }
    timespec_get(&t1, TIME_UTC);
    double secs = (t1.tv_sec - t0.tv_sec) + (t1.tv_nsec - t0.tv_nsec) * 1e-9;
    return secs > 0.0 ? iterations / secs : 0.0;
}

const char* verushash_version(void) {
    return "ZION VerusHash v2.2 — production (Haraka+CLHash from VerusCoin)";
}

} /* extern "C" */
