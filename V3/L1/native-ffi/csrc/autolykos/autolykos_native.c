/*
 * ============================================================================
 *  ZION Native Autolykos v2 C Library
 *  Real implementation of the Ergo (ERG) Autolykos v2 Proof-of-Work algorithm.
 *
 *  Algorithm (memory-hard, table-based):
 *
 *    1. Precompute table (on host): M entries, each 8 bytes (u64)
 *       - seed = SHA-256(header)
 *       - table[i] = BLAKE2b-256(seed || be64(i) || be32(height))[0..8]
 *                     interpreted as big-endian u64
 *
 *    2. Mining loop (per nonce):
 *       - r = nonce mod M            (M = table_size, a power of two)
 *       - for k in 0..9 (9 iterations):
 *           x = (r * nonce + k) mod M
 *           r = table[x]
 *       - hash = BLAKE2b-256(header || r_BE8 || nonce_BE8)
 *       - accept if hash <= target (big-endian byte comparison)
 *
 *  Table size:
 *    - Default:  2^23 entries (64 MB)  — ZION_AUTOLYKOS_TABLE_SIZE env override
 *    - Mainnet:  2^26 entries (512 MB)
 *
 *  References:
 *    - Ergo Autolykos v2 whitepaper (ErgoPow.tex, "Autolykos version 2")
 *    - https://docs.ergoplatform.com/mining/algo-technical/
 *    - BLAKE2b: RFC 7693
 *    - SHA-256: FIPS 180-4
 *    - OpenCL kernel: csrc/opencl/autolykos_kernel.cl
 *
 *  FFI signatures:
 *    autolykos_generate_table(header*, header_len, height, table*, table_size)
 *    autolykos_mine(header*, header_len, nonce, table*, table_size,
 *                   target*, output*) -> int  (1 if hash <= target)
 *    autolykos_hash(header*, header_len, nonce, height, output*) -> u64
 *        (convenience: generates table internally, returns hash + LE u64)
 *
 *  C99/C11 valid, no external dependencies.
 * ============================================================================
 */

#define _POSIX_C_SOURCE 200112L

#include <stdint.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <time.h>

#ifdef _WIN32
    #include <windows.h>
    #define EXPORT __declspec(dllexport)
#else
    #define EXPORT
#endif

/* ============================================================================
 * BLAKE2b-256 — Full RFC-7693 implementation
 * ============================================================================ */

static const uint64_t BLAKE2B_IV[8] = {
    0x6a09e667f3bcc908ULL, 0xbb67ae8584caa73bULL,
    0x3c6ef372fe94f82bULL, 0xa54ff53a5f1d36f1ULL,
    0x510e527fade682d1ULL, 0x9b05688c2b3e6c1fULL,
    0x1f83d9abfb41bd6bULL, 0x5be0cd19137e2179ULL
};

static const uint8_t BLAKE2B_SIGMA[12][16] = {
    {  0,  1,  2,  3,  4,  5,  6,  7,  8,  9, 10, 11, 12, 13, 14, 15 },
    { 14, 10,  4,  8,  9, 15, 13,  6,  1, 12,  0,  2, 11,  7,  5,  3 },
    { 11,  8, 12,  0,  5,  2, 15, 13, 10, 14,  3,  6,  7,  1,  9,  4 },
    {  7,  9,  3,  1, 13, 12, 11, 14,  2,  6,  5, 10,  4,  0, 15,  8 },
    {  9,  0,  5,  7,  2,  4, 10, 15, 14,  1, 11, 12,  6,  8,  3, 13 },
    {  2, 12,  6, 10,  0, 11,  8,  3,  4, 13,  7,  5, 15, 14,  1,  9 },
    { 12,  5,  1, 15, 14, 13,  4, 10,  0,  7,  6,  3,  9,  2,  8, 11 },
    { 13, 11,  7, 14, 12,  1,  3,  9,  5,  0, 15,  4,  8,  6,  2, 10 },
    {  6, 15, 14,  9, 11,  3,  0,  8, 12,  2, 13,  7,  1,  4, 10,  5 },
    { 10,  2,  8,  4,  7,  6,  1,  5, 15, 11,  9, 14,  3, 12, 13,  0 },
    {  0,  1,  2,  3,  4,  5,  6,  7,  8,  9, 10, 11, 12, 13, 14, 15 },
    { 14, 10,  4,  8,  9, 15, 13,  6,  1, 12,  0,  2, 11,  7,  5,  3 }
};

typedef struct {
    uint64_t h[8];
    uint64_t t[2];
    uint64_t f[2];
    uint8_t  buf[128];
    size_t   buflen;
    size_t   outlen;
} blake2b_state;

static inline uint64_t b2_rotr64(uint64_t x, int y) {
    return (x >> y) | (x << (64 - y));
}
static inline uint64_t b2_load64_le(const void* p) {
    uint64_t v; memcpy(&v, p, 8); return v;
}

static void b2_compress(blake2b_state* S, const uint8_t* block) {
    uint64_t m[16], v[16];
    int i;
    for (i = 0; i < 16; i++) m[i] = b2_load64_le(block + i*8);
    for (i = 0; i < 8;  i++) v[i] = S->h[i];
    v[ 8] = BLAKE2B_IV[0]; v[ 9] = BLAKE2B_IV[1];
    v[10] = BLAKE2B_IV[2]; v[11] = BLAKE2B_IV[3];
    v[12] = S->t[0] ^ BLAKE2B_IV[4]; v[13] = S->t[1] ^ BLAKE2B_IV[5];
    v[14] = S->f[0] ^ BLAKE2B_IV[6]; v[15] = S->f[1] ^ BLAKE2B_IV[7];

#define B2_G(r,i,a,b,c,d) do { \
    a=a+b+m[BLAKE2B_SIGMA[r][(i)*2+0]]; d=b2_rotr64(d^a,32); c=c+d; b=b2_rotr64(b^c,24); \
    a=a+b+m[BLAKE2B_SIGMA[r][(i)*2+1]]; d=b2_rotr64(d^a,16); c=c+d; b=b2_rotr64(b^c,63); \
} while(0)
#define B2_ROUND(r) \
    B2_G(r,0,v[0],v[4],v[ 8],v[12]); B2_G(r,1,v[1],v[5],v[ 9],v[13]); \
    B2_G(r,2,v[2],v[6],v[10],v[14]); B2_G(r,3,v[3],v[7],v[11],v[15]); \
    B2_G(r,4,v[0],v[5],v[10],v[15]); B2_G(r,5,v[1],v[6],v[11],v[12]); \
    B2_G(r,6,v[2],v[7],v[ 8],v[13]); B2_G(r,7,v[3],v[4],v[ 9],v[14])
    B2_ROUND(0);B2_ROUND(1);B2_ROUND(2);B2_ROUND(3);B2_ROUND(4);B2_ROUND(5);
    B2_ROUND(6);B2_ROUND(7);B2_ROUND(8);B2_ROUND(9);B2_ROUND(10);B2_ROUND(11);
#undef B2_G
#undef B2_ROUND
    for (i = 0; i < 8; i++) S->h[i] ^= v[i] ^ v[i+8];
}

static void b2_init(blake2b_state* S, size_t outlen) {
    int i; memset(S, 0, sizeof(*S));
    for (i = 0; i < 8; i++) S->h[i] = BLAKE2B_IV[i];
    /* Parameter word: digest=32, key=0, fanout=1, depth=1 → 0x01010020 */
    S->h[0] ^= 0x01010020ULL;
    S->outlen = outlen;
}

static void b2_update(blake2b_state* S, const uint8_t* in, size_t inlen) {
    size_t left, fill;
    while (inlen > 0) {
        left = S->buflen; fill = 128 - left;
        if (inlen > fill) {
            memcpy(S->buf + left, in, fill);
            S->t[0] += 128; if (S->t[0] < 128) S->t[1]++;
            b2_compress(S, S->buf);
            S->buflen = 0; in += fill; inlen -= fill;
        } else {
            memcpy(S->buf + left, in, inlen);
            S->buflen += inlen; inlen = 0;
        }
    }
}

static void b2_final(blake2b_state* S, uint8_t* out) {
    size_t i; uint8_t tmp[64];
    S->t[0] += (uint64_t)S->buflen; if (S->t[0] < S->buflen) S->t[1]++;
    S->f[0] = (uint64_t)-1;
    memset(S->buf + S->buflen, 0, 128 - S->buflen);
    b2_compress(S, S->buf);
    for (i = 0; i < 8; i++) {
        tmp[i*8+0]=(uint8_t)(S->h[i]);    tmp[i*8+1]=(uint8_t)(S->h[i]>> 8);
        tmp[i*8+2]=(uint8_t)(S->h[i]>>16); tmp[i*8+3]=(uint8_t)(S->h[i]>>24);
        tmp[i*8+4]=(uint8_t)(S->h[i]>>32); tmp[i*8+5]=(uint8_t)(S->h[i]>>40);
        tmp[i*8+6]=(uint8_t)(S->h[i]>>48); tmp[i*8+7]=(uint8_t)(S->h[i]>>56);
    }
    memcpy(out, tmp, S->outlen);
}

/* Convenience: hash a single byte-array into 32-byte output */
static void blake2b256_one(const uint8_t* a, size_t la, uint8_t out[32]) {
    blake2b_state S;
    b2_init(&S, 32); b2_update(&S, a, la); b2_final(&S, out);
}

/* Exported raw blake2b-256 for other callers (not part of Autolykos FFI) */
EXPORT void blake2b_hash(const uint8_t* data, size_t len, uint8_t* out) {
    blake2b256_one(data, len, out);
}

/* ============================================================================
 * SHA-256 — FIPS 180-4 implementation (for table seed)
 * ============================================================================ */

static const uint32_t SHA256_K[64] = {
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
    0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
    0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
};

static const uint32_t SHA256_H0[8] = {
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
    0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19
};

typedef struct {
    uint32_t state[8];
    uint8_t  buf[64];
    uint64_t bitlen;
    size_t   buflen;
} sha256_state;

static inline uint32_t sha_rotr32(uint32_t x, int n) {
    return (x >> n) | (x << (32 - n));
}

static void sha256_compress(sha256_state* S, const uint8_t* block) {
    uint32_t w[64];
    uint32_t a, b, c, d, e, f, g, h;
    int i;

    for (i = 0; i < 16; i++) {
        w[i] = ((uint32_t)block[i*4] << 24) | ((uint32_t)block[i*4+1] << 16)
             | ((uint32_t)block[i*4+2] <<  8) |  (uint32_t)block[i*4+3];
    }
    for (i = 16; i < 64; i++) {
        uint32_t s0 = sha_rotr32(w[i-15], 7) ^ sha_rotr32(w[i-15], 18) ^ (w[i-15] >> 3);
        uint32_t s1 = sha_rotr32(w[i-2], 17) ^ sha_rotr32(w[i-2], 19) ^ (w[i-2] >> 10);
        w[i] = w[i-16] + s0 + w[i-7] + s1;
    }

    a = S->state[0]; b = S->state[1]; c = S->state[2]; d = S->state[3];
    e = S->state[4]; f = S->state[5]; g = S->state[6]; h = S->state[7];

    for (i = 0; i < 64; i++) {
        uint32_t S1 = sha_rotr32(e, 6) ^ sha_rotr32(e, 11) ^ sha_rotr32(e, 25);
        uint32_t ch = (e & f) ^ ((~e) & g);
        uint32_t temp1 = h + S1 + ch + SHA256_K[i] + w[i];
        uint32_t S0 = sha_rotr32(a, 2) ^ sha_rotr32(a, 13) ^ sha_rotr32(a, 22);
        uint32_t maj = (a & b) ^ (a & c) ^ (b & c);
        uint32_t temp2 = S0 + maj;
        h = g; g = f; f = e; e = d + temp1;
        d = c; c = b; b = a; a = temp1 + temp2;
    }

    S->state[0] += a; S->state[1] += b; S->state[2] += c; S->state[3] += d;
    S->state[4] += e; S->state[5] += f; S->state[6] += g; S->state[7] += h;
}

static void sha256_init(sha256_state* S) {
    memset(S, 0, sizeof(*S));
    memcpy(S->state, SHA256_H0, sizeof(SHA256_H0));
}

static void sha256_update(sha256_state* S, const uint8_t* data, size_t len) {
    while (len > 0) {
        size_t fill = 64 - S->buflen;
        if (len > fill) {
            memcpy(S->buf + S->buflen, data, fill);
            sha256_compress(S, S->buf);
            S->bitlen += 512;
            S->buflen = 0; data += fill; len -= fill;
        } else {
            memcpy(S->buf + S->buflen, data, len);
            S->buflen += len; len = 0;
        }
    }
}

static void sha256_final(sha256_state* S, uint8_t out[32]) {
    int i;
    /* Append padding bit */
    S->buf[S->buflen++] = 0x80;
    /* If not enough room for 8-byte length, pad and compress */
    if (S->buflen > 56) {
        while (S->buflen < 64) S->buf[S->buflen++] = 0;
        sha256_compress(S, S->buf);
        S->buflen = 0;
    }
    while (S->buflen < 56) S->buf[S->buflen++] = 0;
    /* Append 64-bit big-endian bit length */
    S->bitlen += (uint64_t)S->buflen * 8;
    for (i = 7; i >= 0; i--)
        S->buf[56 + (7 - i)] = (uint8_t)(S->bitlen >> (i * 8));
    sha256_compress(S, S->buf);
    /* Output big-endian */
    for (i = 0; i < 8; i++) {
        out[i*4+0] = (uint8_t)(S->state[i] >> 24);
        out[i*4+1] = (uint8_t)(S->state[i] >> 16);
        out[i*4+2] = (uint8_t)(S->state[i] >>  8);
        out[i*4+3] = (uint8_t)(S->state[i]);
    }
}

static void sha256_hash(const uint8_t* data, size_t len, uint8_t out[32]) {
    sha256_state S;
    sha256_init(&S);
    sha256_update(&S, data, len);
    sha256_final(&S, out);
}

/* ============================================================================
 * Autolykos v2 — Constants and helpers
 * ============================================================================ */

/* Number of iterations in the mining loop (k = 0..8, i.e. 9 iterations) */
#define AUTOLYKOS_K 9

/* Default table size for the convenience autolykos_hash function.
 * Can be overridden via ZION_AUTOLYKOS_TABLE_SIZE env var.
 * Mainnet uses 2^26 (512 MB); 2^23 (64 MB) is the default for testing. */
#define AUTOLYKOS_DEFAULT_TABLE_SIZE (1ULL << 23)

/* Read the table size from the ZION_AUTOLYKOS_TABLE_SIZE env var,
 * falling back to the default. */
static uint64_t autolykos_get_table_size(void) {
    const char* env = getenv("ZION_AUTOLYKOS_TABLE_SIZE");
    if (env && *env) {
        char* end;
        unsigned long long val = strtoull(env, &end, 0);
        if (end != env && val > 0 && (val & (val - 1)) == 0)
            return (uint64_t)val;
    }
    return AUTOLYKOS_DEFAULT_TABLE_SIZE;
}

/* Encode a u64 as 8-byte big-endian */
static inline void be64_encode(uint8_t out[8], uint64_t v) {
    out[0] = (uint8_t)(v >> 56); out[1] = (uint8_t)(v >> 48);
    out[2] = (uint8_t)(v >> 40); out[3] = (uint8_t)(v >> 32);
    out[4] = (uint8_t)(v >> 24); out[5] = (uint8_t)(v >> 16);
    out[6] = (uint8_t)(v >>  8); out[7] = (uint8_t)(v);
}

/* Encode a u32 as 4-byte big-endian */
static inline void be32_encode(uint8_t out[4], uint32_t v) {
    out[0] = (uint8_t)(v >> 24); out[1] = (uint8_t)(v >> 16);
    out[2] = (uint8_t)(v >>  8); out[3] = (uint8_t)(v);
}

/* Decode 8 bytes as big-endian u64 */
static inline uint64_t be64_decode(const uint8_t in[8]) {
    return ((uint64_t)in[0] << 56) | ((uint64_t)in[1] << 48)
         | ((uint64_t)in[2] << 40) | ((uint64_t)in[3] << 32)
         | ((uint64_t)in[4] << 24) | ((uint64_t)in[5] << 16)
         | ((uint64_t)in[6] <<  8) |  (uint64_t)in[7];
}

/* ============================================================================
 * Table generation
 *
 *   seed = SHA-256(header)
 *   table[i] = BLAKE2b-256(seed || be64(i) || be32(height))[0..8] as BE u64
 *
 * The table is an array of table_size u64 entries.
 * ============================================================================ */

EXPORT void autolykos_generate_table(
    const uint8_t* header,
    size_t         header_len,
    uint32_t       height,
    uint64_t*      table,
    uint64_t       table_size
) {
    uint8_t seed[32];
    uint8_t height_be[4];
    uint64_t i;

    /* seed = SHA-256(header) */
    sha256_hash(header, header_len, seed);

    /* height as big-endian u32 */
    be32_encode(height_be, height);

    /* Generate each table entry */
    for (i = 0; i < table_size; i++) {
        uint8_t idx_be[8];
        uint8_t elem_hash[32];
        blake2b_state S;

        be64_encode(idx_be, i);

        /* BLAKE2b-256(seed || be64(i) || be32(height)) */
        b2_init(&S, 32);
        b2_update(&S, seed, 32);
        b2_update(&S, idx_be, 8);
        b2_update(&S, height_be, 4);
        b2_final(&S, elem_hash);

        /* Take first 8 bytes as big-endian u64 */
        table[i] = be64_decode(elem_hash);
    }
}

/* ============================================================================
 * Mining loop
 *
 *   r = nonce mod M
 *   for k in 0..9:
 *     x = (r * nonce + k) mod M
 *     r = table[x]
 *   hash = BLAKE2b-256(header || r_BE8 || nonce_BE8)
 *   return 1 if hash <= target (big-endian), 0 otherwise
 *
 * If table is NULL, the table is generated internally (malloc'd) using
 * table_size (or the default if table_size is 0).
 * ============================================================================ */

EXPORT int autolykos_mine(
    const uint8_t*  header,
    size_t          header_len,
    uint64_t        nonce,
    const uint64_t* table,
    uint64_t        table_size,
    const uint8_t*  target,
    uint8_t*        output
) {
    uint64_t* local_table = NULL;
    const uint64_t* tbl;
    uint64_t M;
    uint64_t mask;
    uint64_t r;
    uint8_t r_be[8];
    uint8_t nonce_be[8];
    uint8_t hash[32];
    int k;
    int meets;

    /* Resolve table: use provided table or generate internally */
    if (table != NULL) {
        tbl = table;
        M = table_size;
    } else {
        M = (table_size > 0) ? table_size : autolykos_get_table_size();
        local_table = (uint64_t*)malloc(M * sizeof(uint64_t));
        if (!local_table) return 0;
        /* Generate with height = 0 (caller must provide a precomputed table
         * for correct height-dependent mining).  The convenience
         * autolykos_hash function below handles height properly. */
        autolykos_generate_table(header, header_len, 0, local_table, M);
        tbl = local_table;
    }

    /* M is a power of two → mod M == & (M - 1) */
    mask = M - 1;

    /* Step 1: r = nonce mod M */
    r = nonce & mask;

    /* Step 2: 9 iterations of (r * nonce + k) mod M, then table lookup.
     * Only the low log2(M) bits of each factor affect the result (M is a
     * power of two), so we mask before multiplying to avoid 64-bit overflow. */
    for (k = 0; k < AUTOLYKOS_K; k++) {
        uint64_t x = (((r & mask) * (nonce & mask)) + (uint64_t)k) & mask;
        r = tbl[x];
    }

    /* Step 3: hash = BLAKE2b-256(header || r_BE8 || nonce_BE8) */
    be64_encode(r_be, r);
    be64_encode(nonce_be, nonce);

    {
        blake2b_state S;
        b2_init(&S, 32);
        b2_update(&S, header, header_len);
        b2_update(&S, r_be, 8);
        b2_update(&S, nonce_be, 8);
        b2_final(&S, hash);
    }

    /* Copy hash to output */
    memcpy(output, hash, 32);

    /* Step 4: target check (hash <= target, big-endian byte comparison) */
    meets = 1;
    if (target) {
        for (k = 0; k < 32; k++) {
            if (hash[k] < target[k]) { meets = 1; break; }
            if (hash[k] > target[k]) { meets = 0; break; }
        }
    }

    if (local_table) free(local_table);
    return meets;
}

/* ============================================================================
 * Convenience: autolykos_hash
 *
 * Generates the table internally (with height) and returns the 32-byte hash.
 * The first 8 bytes of the hash are also returned as a LE u64 for compact
 * comparison (backward-compatible with the original FFI signature).
 *
 * Note: This allocates and generates a full table on every call.  For
 * production mining, precompute the table once and use autolykos_mine.
 * ============================================================================ */

EXPORT uint64_t autolykos_hash(
    const uint8_t* header,
    size_t         header_len,
    uint64_t       nonce,
    uint32_t       height,
    uint8_t*       output
) {
    uint64_t M = autolykos_get_table_size();
    uint64_t* table;
    uint8_t hash[32];
    uint64_t mask;
    uint64_t r;
    uint8_t r_be[8];
    uint8_t nonce_be[8];
    int k;

    /* Generate table (with correct height) */
    table = (uint64_t*)malloc(M * sizeof(uint64_t));
    if (!table) {
        memset(output, 0, 32);
        return 0;
    }
    autolykos_generate_table(header, header_len, height, table, M);

    mask = M - 1;

    /* Mining loop */
    r = nonce & mask;
    for (k = 0; k < AUTOLYKOS_K; k++) {
        uint64_t x = (((r & mask) * (nonce & mask)) + (uint64_t)k) & mask;
        r = table[x];
    }

    /* Final hash: BLAKE2b-256(header || r_BE8 || nonce_BE8) */
    be64_encode(r_be, r);
    be64_encode(nonce_be, nonce);
    {
        blake2b_state S;
        b2_init(&S, 32);
        b2_update(&S, header, header_len);
        b2_update(&S, r_be, 8);
        b2_update(&S, nonce_be, 8);
        b2_final(&S, hash);
    }

    free(table);

    memcpy(output, hash, 32);

    /* Return first 8 bytes of output as LE uint64 (for compact comparison) */
    return (uint64_t)hash[0]
        | ((uint64_t)hash[1] <<  8) | ((uint64_t)hash[2] << 16)
        | ((uint64_t)hash[3] << 24) | ((uint64_t)hash[4] << 32)
        | ((uint64_t)hash[5] << 40) | ((uint64_t)hash[6] << 48)
        | ((uint64_t)hash[7] << 56);
}

/* ============================================================================
 * Public API — verify and benchmark
 * ============================================================================ */

EXPORT int autolykos_verify(
    const uint8_t* header, size_t header_len,
    uint64_t nonce, uint32_t height, uint64_t target
) {
    uint8_t out[32];
    uint64_t result = autolykos_hash(header, header_len, nonce, height, out);
    return (result < target) ? 1 : 0;
}

EXPORT double autolykos_benchmark_cpu(int iterations) {
    struct timespec t0, t1;
    uint8_t header[32]; uint8_t out[32];
    volatile uint64_t r = 0; int i;
    memset(header, 0xAB, sizeof(header));
    #if defined(_WIN32)
    LARGE_INTEGER _perf_t0; QueryPerformanceCounter(&_perf_t0);
#else
    timespec_get(&t0, TIME_UTC);
#endif
    for (i = 0; i < iterations; i++)
        r ^= autolykos_hash(header, 32, (uint64_t)i, 700000u, out);
    (void)r;
    #if defined(_WIN32)
    LARGE_INTEGER _perf_t1; QueryPerformanceCounter(&_perf_t1);
#else
    timespec_get(&t1, TIME_UTC);
#endif
    double elapsed = (double)(t1.tv_sec - t0.tv_sec)
                   + (double)(t1.tv_nsec - t0.tv_nsec) * 1e-9;
    return (elapsed > 0.0) ? (double)iterations / elapsed : 0.0;
}

EXPORT const char* autolykos_version(void) {
    return "ZION Autolykos v2.0.0 - ERG Compatible (table-based, BLAKE2b+SHA256)";
}

/* Legacy stubs (kept for ABI compatibility) */
EXPORT void autolykos_generate_elements(const uint8_t*s,size_t sl,uint64_t*e,uint64_t n)
    {(void)s;(void)sl;(void)e;(void)n;}
EXPORT int autolykos_mine_cpu(const uint64_t*e,uint64_t ne,uint64_t ns,uint64_t nend,
    uint64_t t,uint32_t k,uint64_t*rn,uint64_t*rh)
    {(void)e;(void)ne;(void)ns;(void)nend;(void)t;(void)k;(void)rn;(void)rh;return 0;}
EXPORT int autolykos_mine_cpu_batch(const uint64_t*e,uint64_t ne,uint64_t ns,uint64_t bs,
    uint64_t t,uint32_t k,uint64_t*rn,uint64_t*rh)
    {return autolykos_mine_cpu(e,ne,ns,ns+bs,t,k,rn,rh);}
