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
#if defined(_WIN32)
#include <windows.h>
#endif
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

/*
 * Compute VerusHash v2.2 of a complete block header (no nonce appended).
 * The caller is responsible for embedding the nonce in the header's nonce
 * field (offset 108, 32 bytes) and/or the solution's nonceSpace before
 * calling this function.  Writes exactly 32 bytes to output.
 *
 * This is the correct entry point for VRSC merge-mining where the full
 * 1487-byte block header (including the 32-byte nonce field and the
 * 1344-byte solution with embedded nonceSpace) is hashed as-is.
 */
void verushash_hash_raw(
    const uint8_t* header,
    size_t         header_len,
    uint8_t*       output)
{
    CVerusHashV2 hasher(SOLUTION_VERUSHHASH_V2_2);
    hasher.Reset();
    hasher.Write(header, header_len);
    hasher.Finalize2b(output);
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
#if defined(_WIN32)
    LARGE_INTEGER _perf0, _perf1; QueryPerformanceCounter(&_perf0);
#else
    timespec_get(&t0, TIME_UTC);
#endif
    for (int32_t i = 0; i < iterations; i++) {
        header[0] = (uint8_t)(i & 0xFF);
        verushash_hash(header, 76, (uint64_t)i, out);
    }
#if defined(_WIN32)
    QueryPerformanceCounter(&_perf1); double _elapsed = (double)(_perf1.QuadPart - _perf0.QuadPart);
#else
    timespec_get(&t1, TIME_UTC); double _elapsed = (t1.tv_sec - t0.tv_sec) + (t1.tv_nsec - t0.tv_nsec) / 1e9;
#endif
    double secs = (t1.tv_sec - t0.tv_sec) + (t1.tv_nsec - t0.tv_nsec) * 1e-9;
    return secs > 0.0 ? iterations / secs : 0.0;
}

const char* verushash_version(void) {
    return "ZION VerusHash v2.2 — production (Haraka+CLHash from VerusCoin)";
}

/* ====================================================================
 * Two-stage mining hash (optimized path — 50-100x faster per nonce)
 *
 * Based on bloxminer/ccminer approach:
 *   1. verushash_hash_half()  — Haraka512 chain → 64-byte intermediate (ONCE per job)
 *   2. verushash_prepare_key() — GenNewCLKey from intermediate (ONCE per job)
 *   3. verushash_hash_with_nonce() — CLHash + final Haraka512 (PER NONCE)
 *
 * Without this, the miner does ~324 Haraka calls per nonce (full hash).
 * With this, only ~2 Haraka calls per nonce.
 * ==================================================================== */

/* Thread-local hasher state for two-stage mining.
 * Each mining thread must call hash_half + prepare_key once per job,
 * then hash_with_nonce for each nonce. */
static thread_local CVerusHashV2* tl_hasher = nullptr;
static thread_local bool tl_key_prepared = false;

/* Pre-computed curBuf constants (computed once in prepare_key, reused per nonce).
 * curBuf layout: [0..31]=intermediate, [32..46]=nonceSpace, [47]=ch, [48..63]=fill1
 * Only [32..46] varies per nonce; [47..63] is overwritten by FillExtra after CLHash
 * and restored from fill1/ch at the start of each hash_with_nonce call. */
alignas(32) static thread_local uint8_t tl_curBuf[64];
static thread_local __m128i tl_fill1;
static thread_local uint8_t tl_ch;
static thread_local __m128i tl_shuf2;

/* fixupkey: restore only the ~64 modified 16-byte key blocks from the refresh area.
 * This replaces the old 8832-byte memcpy per nonce with ~1024 bytes of restores.
 * Matches the VerusCoin reference miner (verus_clhash.cpp mine_verus_v2). */
static inline __attribute__((always_inline)) void fixupkey_opt(
    __m128i **pMoveScratch, verusclhash_descr *pdesc)
{
    uint32_t ofs = pdesc->keySizeInBytes >> 4;
    for (__m128i *pfixup = *pMoveScratch; pfixup; pfixup = *++pMoveScratch)
    {
        const __m128i fixup = _mm_load_si128((__m128i *)(pfixup + ofs));
        _mm_store_si128((__m128i *)pfixup, fixup);
    }
}

/* Stage 1: Compute 64-byte intermediate state from full block data.
 * Matches ccminer's VerusHashHalf / bloxminer's hash_half().
 * Caller provides intermediate64 buffer of at least 64 bytes. */
void verushash_hash_half(
    const uint8_t* data,
    size_t         data_len,
    uint8_t*       intermediate64)
{
    if (!tl_hasher) {
        tl_hasher = new CVerusHashV2(SOLUTION_VERUSHHASH_V2_2);
    }

    /* Haraka512 chain hash over the full block data */
    alignas(32) unsigned char buf1[64] = {0};
    alignas(32) unsigned char buf2[64];
    unsigned char* curBuf = buf1;
    unsigned char* result = buf2;
    size_t curPos = 0;

    const unsigned char* ptr = data;
    for (size_t pos = 0; pos < data_len; ) {
        size_t room = 32 - curPos;
        if (data_len - pos >= room) {
            memcpy(curBuf + 32 + curPos, ptr + pos, room);
            (*CVerusHashV2::haraka512Function)(result, curBuf);
            unsigned char* tmp = curBuf;
            curBuf = result;
            result = tmp;
            pos += room;
            curPos = 0;
        } else {
            memcpy(curBuf + 32 + curPos, ptr + pos, data_len - pos);
            curPos += data_len - pos;
            pos = data_len;
        }
    }

    /* FillExtra — matches ccminer:
     *   memcpy(curBuf + 47, curBuf, 16);
     *   memcpy(curBuf + 63, curBuf, 1); */
    memcpy(curBuf + 47, curBuf, 16);
    memcpy(curBuf + 63, curBuf, 1);

    /* Return the 64-byte intermediate state */
    memcpy(intermediate64, curBuf, 64);
}

/* Stage 2: Generate CLHash key from intermediate state.
 * Must be called once per job after hash_half.
 * Pre-computes curBuf constants (fill1, ch, shuf2) for reuse per nonce. */
void verushash_prepare_key(const uint8_t* intermediate64)
{
    if (!tl_hasher) {
        tl_hasher = new CVerusHashV2(SOLUTION_VERUSHHASH_V2_2);
    }

    /* GenNewCLKey uses the first 32 bytes of intermediate as seed.
     * This also sets up the refresh area (key+keySize has a copy of the
     * first keyrefreshsize bytes) and zeroes the pMoveScratch area. */
    u128* key = CVerusHashV2::GenNewCLKey((unsigned char*)intermediate64);

    /* Pre-compute curBuf constant parts from intermediate:
     *   curBuf[0..31]  = intermediate[0..31]  (constant across nonces)
     *   curBuf[47]     = intermediate[0]      (ch, constant)
     *   curBuf[48..63] = shuffle(intermediate[0..15], shuf1)  (fill1, constant)
     *   curBuf[32..46] = nonceSpace (varies per nonce, set in hash_with_nonce) */
    memcpy(tl_curBuf, intermediate64, 64);

    const __m128i shuf1 = _mm_setr_epi8(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 0);
    tl_shuf2 = _mm_setr_epi8(1, 2, 3, 4, 5, 6, 7, 0, 1, 2, 3, 4, 5, 6, 7, 0);

    __m128i src = _mm_load_si128((const __m128i*)tl_curBuf);
    tl_fill1 = _mm_shuffle_epi8(src, shuf1);
    tl_ch = tl_curBuf[0];

    /* Store fill1 and ch into curBuf (will be overwritten by FillExtra after
     * CLHash, then restored at the start of each hash_with_nonce call) */
    _mm_store_si128((__m128i*)(tl_curBuf + 48), tl_fill1);
    tl_curBuf[47] = tl_ch;

    tl_key_prepared = (key != nullptr);
}

/* Stage 3: Compute final 32-byte hash from intermediate + 15-byte nonceSpace.
 * Called for each nonce iteration. prepare_key must have been called first.
 *
 * Uses fixupkey() to restore only the ~64 modified key blocks (1024 bytes)
 * from the refresh area, instead of a full 8832-byte memcpy per nonce.
 * Also uses pre-computed curBuf constants from prepare_key.
 *
 * Matches the VerusCoin reference miner (verus_clhash.cpp mine_verus_v2). */
void verushash_hash_with_nonce(
    const uint8_t* intermediate64,
    const uint8_t* nonceSpace15,
    uint8_t*       output)
{
    if (!tl_key_prepared) {
        memset(output, 0, 32);
        return;
    }

    /* Get current key from thread-local hasher */
    u128* key = (u128*)verusclhasher_key.get();
    if (!key) {
        memset(output, 0, 32);
        return;
    }

    verusclhash_descr* pdesc = (verusclhash_descr*)verusclhasher_descr.get();
    size_t keySize = pdesc ? pdesc->keySizeInBytes : 8832;
    uint64_t keyMask = tl_hasher->vclh.keyMask;

    /* Restore key using fixupkey — only ~64 modified 16-byte blocks from
     * the refresh area (1024 bytes) vs full 8832-byte memcpy.
     * On the first call after prepare_key, pMoveScratch is all zeros
     * (zeroed by GenNewCLKey), so fixupkey is a no-op. */
    uint64_t keyrefreshsize = keyMask + 1;
    __m128i** pMoveScratch = (__m128i**)((uint8_t*)key + keySize + keyrefreshsize);
    fixupkey_opt(pMoveScratch, pdesc);

    /* Restore curBuf[47..63] from pre-computed fill1/ch (overwritten by
     * the previous call's FillExtra with CLHash result) */
    _mm_store_si128((__m128i*)(tl_curBuf + 48), tl_fill1);
    tl_curBuf[47] = tl_ch;

    /* Copy the 15-byte nonceSpace to positions 32-46 */
    memcpy(tl_curBuf + 32, nonceSpace15, 15);

    /* Run CLHash v2.2 */
    __m128i acc = __verusclmulwithoutreduction64alignedrepeat_sv2_2(
        (__m128i*)key, (const __m128i*)tl_curBuf, keyMask, pMoveScratch);

    /* Finish CLHash — reduction + GF(2^128) division */
    const __m128i lengthvector = _mm_set_epi64x(1024, 64);
    const __m128i clprod1 = _mm_clmulepi64_si128(lengthvector, lengthvector, 0x10);
    acc = _mm_xor_si128(acc, clprod1);

    const __m128i C = _mm_cvtsi64_si128((1U<<4)+(1U<<3)+(1U<<1)+(1U<<0));
    __m128i Q2 = _mm_clmulepi64_si128(acc, C, 0x01);
    __m128i Q3 = _mm_shuffle_epi8(
        _mm_setr_epi8(0, 27, 54, 45, 108, 119, 90, 65,
                      (char)216, (char)195, (char)238, (char)245,
                      (char)180, (char)175, (char)130, (char)153),
        _mm_srli_si128(Q2, 8));
    __m128i Q4 = _mm_xor_si128(Q2, acc);
    acc = _mm_xor_si128(Q3, Q4);
    uint64_t intermediate = _mm_cvtsi128_si64(acc);

    /* FillExtra with CLHash result */
    __m128i intVec = _mm_loadl_epi64((const __m128i*)&intermediate);
    __m128i fill2 = _mm_shuffle_epi8(intVec, tl_shuf2);
    _mm_store_si128((__m128i*)(tl_curBuf + 48), fill2);
    tl_curBuf[47] = ((const uint8_t*)&intermediate)[0];

    /* Mask for key offset */
    uint64_t keyOffset = intermediate & (keyMask >> 4);

    /* Final keyed Haraka512 */
    (*CVerusHashV2::haraka512KeyedFunction)(output, tl_curBuf, key + keyOffset);
}

/* Cleanup thread-local two-stage state (called when thread exits or
 * when a new job requires re-initialization). */
void verushash_mining_reset(void) {
    tl_key_prepared = false;
    /* Don't delete tl_hasher — it's reused across jobs. */
}

/* ====================================================================
 * Batch nonce scan — the entire nonce loop runs in C++, eliminating
 * per-nonce Rust→C++ FFI overhead (~2 calls/hash saved).
 *
 * verushash_scan_nonces() takes a nonce range and target, returns the
 * winning nonce + hash, or -1 if none found.
 *
 * Prerequisites: hash_half + prepare_key must have been called for this
 * job on this thread.
 * ==================================================================== */

/* Target comparison: VerusHash v2.2 returns hash in LE byte order.
 * Target is BE. We compare hash_reversed (→BE) vs target (BE).
 * Returns 1 if hash <= target, 0 otherwise. */
static inline __attribute__((always_inline)) int meets_target_le(
    const uint8_t hash[32], const uint8_t target[32])
{
    /* Compare reversed hash (LE→BE) against target (BE) */
    for (int i = 31; i >= 0; i--) {
        if (hash[i] < target[31 - i]) return 1;
        if (hash[i] > target[31 - i]) return 0;
    }
    return 1; /* equal */
}

int64_t verushash_scan_nonces(
    const uint8_t* intermediate64,
    const uint8_t* nonceSpace15_template,  /* 15 bytes: en1 + zeros */
    uint32_t       nonce_offset,            /* offset of 4-byte miner nonce in nonceSpace */
    uint64_t       start_nonce,
    uint64_t       end_nonce,
    const uint8_t  target[32],
    uint8_t*       out_hash,                /* 32 bytes — winning hash */
    uint64_t*      out_nonce)               /* winning nonce */
{
    if (!tl_key_prepared) {
        return -1;
    }

    u128* key = (u128*)verusclhasher_key.get();
    if (!key) return -1;

    verusclhash_descr* pdesc = (verusclhash_descr*)verusclhasher_descr.get();
    size_t keySize = pdesc ? pdesc->keySizeInBytes : 8832;
    uint64_t keyMask = tl_hasher->vclh.keyMask;
    uint64_t keyrefreshsize = keyMask + 1;
    __m128i** pMoveScratch = (__m128i**)((uint8_t*)key + keySize + keyrefreshsize);

    /* Pre-compute constant shuf masks */
    const __m128i lengthvector = _mm_set_epi64x(1024, 64);
    const __m128i C = _mm_cvtsi64_si128((1U<<4)+(1U<<3)+(1U<<1)+(1U<<0));
    const __m128i shuf3 = _mm_setr_epi8(0, 27, 54, 45, 108, 119, 90, 65,
                                        (char)216, (char)195, (char)238, (char)245,
                                        (char)180, (char)175, (char)130, (char)153);

    /* Local nonceSpace — copy template, update 4 bytes per nonce */
    uint8_t ns[15];
    memcpy(ns, nonceSpace15_template, 15);

    alignas(32) uint8_t hash[32];

    for (uint64_t nonce = start_nonce; nonce < end_nonce; nonce++) {
        /* Update miner nonce in nonceSpace */
        uint32_t n32 = (uint32_t)nonce;
        if (nonce_offset + 4 <= 15) {
            memcpy(ns + nonce_offset, &n32, 4);
        }

        /* fixupkey — restore modified key blocks from refresh area */
        fixupkey_opt(pMoveScratch, pdesc);

        /* Restore curBuf[47..63] from pre-computed fill1/ch */
        _mm_store_si128((__m128i*)(tl_curBuf + 48), tl_fill1);
        tl_curBuf[47] = tl_ch;

        /* Copy nonceSpace to curBuf[32..46] */
        memcpy(tl_curBuf + 32, ns, 15);

        /* CLHash v2.2 */
        __m128i acc = __verusclmulwithoutreduction64alignedrepeat_sv2_2(
            (__m128i*)key, (const __m128i*)tl_curBuf, keyMask, pMoveScratch);

        /* Reduction + GF(2^128) division */
        const __m128i clprod1 = _mm_clmulepi64_si128(lengthvector, lengthvector, 0x10);
        acc = _mm_xor_si128(acc, clprod1);

        __m128i Q2 = _mm_clmulepi64_si128(acc, C, 0x01);
        __m128i Q3 = _mm_shuffle_epi8(shuf3, _mm_srli_si128(Q2, 8));
        __m128i Q4 = _mm_xor_si128(Q2, acc);
        acc = _mm_xor_si128(Q3, Q4);
        uint64_t intermediate = _mm_cvtsi128_si64(acc);

        /* FillExtra with CLHash result */
        __m128i intVec = _mm_loadl_epi64((const __m128i*)&intermediate);
        __m128i fill2 = _mm_shuffle_epi8(intVec, tl_shuf2);
        _mm_store_si128((__m128i*)(tl_curBuf + 48), fill2);
        tl_curBuf[47] = ((const uint8_t*)&intermediate)[0];

        /* Final keyed Haraka512 */
        uint64_t keyOffset = intermediate & (keyMask >> 4);
        (*CVerusHashV2::haraka512KeyedFunction)(hash, tl_curBuf, key + keyOffset);

        /* Target check — LE hash vs BE target */
        if (meets_target_le(hash, target)) {
            memcpy(out_hash, hash, 32);
            *out_nonce = nonce;
            return 0; /* found */
        }
    }

    return -1; /* not found */
}

/* Extract the precomputed VerusHash key (8832 bytes) and blockhash_half (64 bytes)
 * for GPU kernel upload. Must be called after verushash_prepare_key().
 * key_out must be 8832 bytes; blockhash_half_out must be 64 bytes.
 * Returns 0 on success, -1 if key not prepared. */
int32_t verushash_get_gpu_keydata(
    uint8_t* key_out,
    uint8_t* blockhash_half_out)
{
    if (!tl_key_prepared) {
        return -1;
    }
    u128* key = (u128*)verusclhasher_key.get();
    if (!key) {
        return -1;
    }
    verusclhash_descr* pdesc = (verusclhash_descr*)verusclhasher_descr.get();
    size_t keySize = pdesc ? pdesc->keySizeInBytes : 8832;
    memcpy(key_out, key, keySize);
    memcpy(blockhash_half_out, tl_curBuf, 64);
    return 0;
}

} /* extern "C" */
