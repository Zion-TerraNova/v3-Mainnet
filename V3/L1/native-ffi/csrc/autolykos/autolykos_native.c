/*
 * ============================================================================
 *  ZION Native Autolykos v2 C Library
 *  Correct implementation based on official ergoplatform/ergo source:
 *    ergo-core/src/main/scala/org/ergoplatform/mining/AutolykosPowScheme.scala
 *
 *  Algorithm (hitForVersion2ForMessage):
 *    M      = concat of i=0..1023 encoded as big-endian int64 (8192 bytes)
 *    nonce  = 8 bytes big-endian from u64 nonce
 *
 *    1. prei8 = Blake2b256(msg || nonce_BE8).takeRight(8)  as unsigned BE u64
 *    2. i4    = (prei8 mod N)  as 4-byte big-endian
 *    3. f31   = Blake2b256(i4 || height_BE4 || M).drop(1)  (31 bytes)
 *    4. seed  = f31 || msg || nonce_BE8
 *    5. genIndexes(seed, N):
 *         h32 = Blake2b256(seed)
 *         ext = h32 || h32[0..2]   (35 bytes)
 *         indices[i] = BigInt(1, ext[i..i+4]) mod N   for i in 0..k
 *    6. element[j] = BigInt(Blake2b256(j_BE4 || height_BE4 || M)[1:])  (31 bytes)
 *    7. f2 = sum(element[j] for j in indices)
 *    8. output = Blake2b256(f2_as_32bytes)
 *    9. return BigInt(1, output)  (as first 8 LE bytes in uint64)
 *
 *  Note: M, element computation with full 8192-byte M is expensive but correct.
 *        For CPU mining (low rate) this is fine.
 *
 *  FFI signature matches L1/cosmic-harmony/src/native_ffi.rs:
 *    autolykos_hash(header*, header_len, nonce: u64, height: u32, output*) -> u64
 * ============================================================================
 */

#define _POSIX_C_SOURCE 200112L

#include <stdint.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <time.h>

#ifdef _WIN32
    #define EXPORT __declspec(dllexport)
#else
    #define EXPORT
#endif

/* ============================================================================
 * Blake2b-256 — Full RFC-7693 implementation
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

#define G(r,i,a,b,c,d) do { \
    a=a+b+m[BLAKE2B_SIGMA[r][(i)*2+0]]; d=b2_rotr64(d^a,32); c=c+d; b=b2_rotr64(b^c,24); \
    a=a+b+m[BLAKE2B_SIGMA[r][(i)*2+1]]; d=b2_rotr64(d^a,16); c=c+d; b=b2_rotr64(b^c,63); \
} while(0)
#define ROUND(r) \
    G(r,0,v[0],v[4],v[ 8],v[12]); G(r,1,v[1],v[5],v[ 9],v[13]); \
    G(r,2,v[2],v[6],v[10],v[14]); G(r,3,v[3],v[7],v[11],v[15]); \
    G(r,4,v[0],v[5],v[10],v[15]); G(r,5,v[1],v[6],v[11],v[12]); \
    G(r,6,v[2],v[7],v[ 8],v[13]); G(r,7,v[3],v[4],v[ 9],v[14])
    ROUND(0);ROUND(1);ROUND(2);ROUND(3);ROUND(4);ROUND(5);
    ROUND(6);ROUND(7);ROUND(8);ROUND(9);ROUND(10);ROUND(11);
#undef G
#undef ROUND
    for (i = 0; i < 8; i++) S->h[i] ^= v[i] ^ v[i+8];
}

static void b2_init(blake2b_state* S, size_t outlen) {
    int i; memset(S, 0, sizeof(*S));
    for (i = 0; i < 8; i++) S->h[i] = BLAKE2B_IV[i];
    S->h[0] ^= 0x01010000ULL ^ (uint8_t)outlen;
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

/* Convenience: hash one or two byte-arrays concatenated */
static void blake2b256_one(const uint8_t* a, size_t la, uint8_t out[32]) {
    blake2b_state S;
    b2_init(&S, 32); b2_update(&S, a, la); b2_final(&S, out);
}
static void blake2b256_two(const uint8_t* a, size_t la,
                            const uint8_t* b, size_t lb, uint8_t out[32]) {
    blake2b_state S;
    b2_init(&S, 32); b2_update(&S, a, la); b2_update(&S, b, lb); b2_final(&S, out);
}
static void blake2b256_three(const uint8_t* a, size_t la,
                              const uint8_t* b, size_t lb,
                              const uint8_t* c, size_t lc, uint8_t out[32]) {
    blake2b_state S;
    b2_init(&S, 32);
    b2_update(&S, a, la); b2_update(&S, b, lb); b2_update(&S, c, lc);
    b2_final(&S, out);
}

/* Exported raw blake2b for other callers */
EXPORT void blake2b_hash(const uint8_t* data, size_t len, uint8_t* out) {
    blake2b256_one(data, len, out);
}

/* ============================================================================
 * Autolykos v2 Constants
 * ============================================================================ */

#define AUTOLYKOS_K 32

/*
 * M = concat of i=0..1023 as big-endian int64 → 8192 bytes.
 * Generated at runtime by build_M().
 */
static uint8_t M_ARRAY[8192];
static int     M_READY = 0;

static void build_M(void) {
    int i, j;
    if (M_READY) return;
    for (i = 0; i < 1024; i++) {
        uint64_t v = (uint64_t)i;
        uint8_t* p = M_ARRAY + i * 8;
        /* big-endian int64 (Java/Scala Longs.toByteArray) */
        for (j = 7; j >= 0; j--) { p[j] = (uint8_t)(v & 0xFF); v >>= 8; }
    }
    M_READY = 1;
}

/* N grows ~5% per 50*1024 blocks after height 614400 */
static uint32_t calcN(uint32_t height) {
    uint32_t base = (1u << 26);
    if (height < 614400) return base;
    uint32_t iters = (height - 614400) / (50 * 1024) + 1;
    uint32_t N = base;
    uint32_t i;
    for (i = 0; i < iters; i++) N = N / 100 * 105;
    return N;
}

/* ============================================================================
 * 32-byte big-endian BigInt helpers
 * ============================================================================ */

/* Add src (31 or 32-byte big-endian) into dst (32-byte big-endian), wrapping mod 2^256 */
static void bigint32_add(uint8_t dst[32], const uint8_t* src, size_t srclen) {
    uint32_t carry = 0;
    int i;
    /* align src to the right (it may be 31 bytes) */
    int offset = 32 - (int)srclen;
    for (i = 31; i >= 0; i--) {
        uint32_t sv = (i >= offset) ? src[i - offset] : 0;
        uint32_t sum = (uint32_t)dst[i] + sv + carry;
        dst[i] = (uint8_t)(sum & 0xFF);
        carry  = sum >> 8;
    }
    /* ignore overflow (mod 2^256) */
}

/* ============================================================================
 * Core Autolykos v2 hash — matches hitForVersion2ForMessage
 * ============================================================================ */

/*
 * autolykos_hash:
 *   header     = msg bytes (the 32-byte message from pool notify params[2])
 *   header_len = length of header (typically 32)
 *   nonce      = 64-bit nonce
 *   height     = block height
 *   output     = 32-byte result (Blake2b256 of element-sum)
 *   returns    = first 8 bytes of output as LE uint64 (for compact comparison)
 */
EXPORT uint64_t autolykos_hash(
    const uint8_t* header,
    size_t         header_len,
    uint64_t       nonce,
    uint32_t       height,
    uint8_t*       output
) {
    int i, k;
    uint8_t tmp[32], ext[35], seed_buf[8192 + 32 + 8 + 32]; /* generous */
    size_t  seed_len;
    uint32_t N = calcN(height);

    build_M();

    /* nonce as 8-byte big-endian (Scala: Longs.toByteArray) */
    uint8_t nonce_be[8];
    nonce_be[0] = (uint8_t)(nonce >> 56); nonce_be[1] = (uint8_t)(nonce >> 48);
    nonce_be[2] = (uint8_t)(nonce >> 40); nonce_be[3] = (uint8_t)(nonce >> 32);
    nonce_be[4] = (uint8_t)(nonce >> 24); nonce_be[5] = (uint8_t)(nonce >> 16);
    nonce_be[6] = (uint8_t)(nonce >>  8); nonce_be[7] = (uint8_t)(nonce);

    /* height as 4-byte big-endian */
    uint8_t height_be[4];
    height_be[0] = (uint8_t)(height >> 24); height_be[1] = (uint8_t)(height >> 16);
    height_be[2] = (uint8_t)(height >>  8); height_be[3] = (uint8_t)(height);

    /* Step 1: prei8 = Blake2b256(msg || nonce_BE8).takeRight(8) as uint64 */
    uint8_t h1[32];
    blake2b256_two(header, header_len, nonce_be, 8, h1);
    uint64_t prei8 = 0;
    for (i = 24; i < 32; i++) prei8 = (prei8 << 8) | h1[i];

    /* Step 2: i4 = (prei8 mod N) as 4-byte big-endian */
    uint32_t idx_val = (uint32_t)(prei8 % (uint64_t)N);
    uint8_t i4[4];
    i4[0] = (uint8_t)(idx_val >> 24); i4[1] = (uint8_t)(idx_val >> 16);
    i4[2] = (uint8_t)(idx_val >>  8); i4[3] = (uint8_t)(idx_val);

    /* Step 3: f = Blake2b256(i4 || height_be4 || M).drop(1) → 31 bytes */
    {
        blake2b_state S;
        b2_init(&S, 32);
        b2_update(&S, i4, 4);
        b2_update(&S, height_be, 4);
        b2_update(&S, M_ARRAY, 8192);
        b2_final(&S, tmp);
    }
    /* f31 = tmp[1..31] (31 bytes, drop first byte = takeRight(31)) */
    const uint8_t* f31 = tmp + 1;  /* 31 bytes */

    /* Step 4: seed = f31 || msg || nonce_BE8 */
    memcpy(seed_buf, f31, 31);
    memcpy(seed_buf + 31, header, header_len);
    memcpy(seed_buf + 31 + header_len, nonce_be, 8);
    seed_len = 31 + header_len + 8;

    /* Step 5: genIndexes(seed, N) */
    uint8_t hash32[32];
    blake2b256_one(seed_buf, seed_len, hash32);
    /* ext = hash32 || hash32[0..2] */
    memcpy(ext, hash32, 32);
    memcpy(ext + 32, hash32, 3);

    uint32_t indices[AUTOLYKOS_K];
    for (k = 0; k < AUTOLYKOS_K; k++) {
        uint32_t raw = ((uint32_t)ext[k] << 24) | ((uint32_t)ext[k+1] << 16)
                     | ((uint32_t)ext[k+2] <<  8) |  (uint32_t)ext[k+3];
        indices[k] = raw % N;
    }

    /* Step 6+7: elems, sum */
    /* Each element = Blake2b256(idx_BE4 || height_be4 || M).drop(1) = 31 bytes as BigInt */
    uint8_t f2[32];
    memset(f2, 0, 32);

    for (k = 0; k < AUTOLYKOS_K; k++) {
        uint8_t j4[4];
        uint8_t elem_hash[32];
        j4[0] = (uint8_t)(indices[k] >> 24); j4[1] = (uint8_t)(indices[k] >> 16);
        j4[2] = (uint8_t)(indices[k] >>  8); j4[3] = (uint8_t)(indices[k]);
        {
            blake2b_state S;
            b2_init(&S, 32);
            b2_update(&S, j4, 4);
            b2_update(&S, height_be, 4);
            b2_update(&S, M_ARRAY, 8192);
            b2_final(&S, elem_hash);
        }
        /* elem = elem_hash[1..31] (31 bytes), add to f2 as big-endian bigint */
        bigint32_add(f2, elem_hash + 1, 31);
    }

    /* Step 8: output = Blake2b256(f2_32bytes) */
    blake2b256_one(f2, 32, output);

    /* Return first 8 bytes of output as LE uint64 */
    return (uint64_t)output[0]
        | ((uint64_t)output[1] <<  8) | ((uint64_t)output[2] << 16)
        | ((uint64_t)output[3] << 24) | ((uint64_t)output[4] << 32)
        | ((uint64_t)output[5] << 40) | ((uint64_t)output[6] << 48)
        | ((uint64_t)output[7] << 56);
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
    timespec_get(&t0, TIME_UTC);
    for (i = 0; i < iterations; i++)
        r ^= autolykos_hash(header, 32, (uint64_t)i, 700000u, out);
    (void)r;
    timespec_get(&t1, TIME_UTC);
    double elapsed = (double)(t1.tv_sec - t0.tv_sec)
                   + (double)(t1.tv_nsec - t0.tv_nsec) * 1e-9;
    return (elapsed > 0.0) ? (double)iterations / elapsed : 0.0;
}

/* Legacy stubs */
EXPORT void autolykos_generate_elements(const uint8_t*s,size_t sl,uint64_t*e,uint64_t n)
    {(void)s;(void)sl;(void)e;(void)n;}
EXPORT int autolykos_mine_cpu(const uint64_t*e,uint64_t ne,uint64_t ns,uint64_t nend,
    uint64_t t,uint32_t k,uint64_t*rn,uint64_t*rh)
    {(void)e;(void)ne;(void)ns;(void)nend;(void)t;(void)k;(void)rn;(void)rh;return 0;}
EXPORT int autolykos_mine_cpu_batch(const uint64_t*e,uint64_t ne,uint64_t ns,uint64_t bs,
    uint64_t t,uint32_t k,uint64_t*rn,uint64_t*rh)
    {return autolykos_mine_cpu(e,ne,ns,ns+bs,t,k,rn,rh);}
