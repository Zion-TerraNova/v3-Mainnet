/*
 * ZION Cosmic Harmony Deeksha — Canonical CUDA GPU Mining Kernel
 *
 * Pipeline (EXACT match to Rust cosmic_harmony_deeksha()):
 *   Step 1: Keccak-256 (header||nonce → 32 B)   [pad 0x01, rate 136]
 *   Step 2: SHA3-512   (32 B → 64 B)            [pad 0x06, rate 72]
 *   Step 3: Golden Matrix (φ^k fixed-point, 64 B → 64 B)
 *   Step 4: Memory-Hard (64 KiB scratchpad, 2 sequential passes, 64 random reads)
 *   Step 5: NPU Mix (INT8 MLP 64→128→64 + residual)
 *   Step 6: Cosmic Fusion (4 × Keccak-256 + AES-128 + XOR, final SHA3-512 → 32 B)
 *
 * Ported from the canonical OpenCL kernel (cosmic_harmony_deeksha.cl).
 * Every operation is bit-exact with Rust reference and OpenCL production kernel.
 *
 * Author: ZION AI Native Team
 * Version: 2.9.8 — Canonical Deeksha CUDA GPU
 */

typedef unsigned char      uint8_t;
typedef signed char        int8_t;
typedef unsigned short     uint16_t;
typedef short              int16_t;
typedef unsigned int       uint32_t;
typedef int                int32_t;
typedef unsigned long long uint64_t;
typedef long long          int64_t;

/* ========================================================================== */
/* Constants                                                                   */
/* ========================================================================== */

#define SCRATCHPAD_SIZE  262144
#define BLOCK_SIZE       64
#define BLOCK_COUNT      4096
#define PASSES           4
#define RANDOM_READS     256
#define MATRIX_DIM       8

#define ROL64(x, n) (((x) << (n)) | ((x) >> (64 - (n))))
#define XTIME(a) ((uint8_t)(((a) << 1) ^ ((((a) >> 7) & 1) * 0x1b)))

/* Chi macro: one 5-element row */
#define CHI_ROW(b) \
{ uint64_t _a=st[(b)],_b=st[(b)+1],_c=st[(b)+2],_d=st[(b)+3],_e=st[(b)+4]; \
  st[(b)]    = _a ^ ((~_b) & _c); \
  st[(b)+1]  = _b ^ ((~_c) & _d); \
  st[(b)+2]  = _c ^ ((~_d) & _e); \
  st[(b)+3]  = _d ^ ((~_e) & _a); \
  st[(b)+4]  = _e ^ ((~_a) & _b); }

/* Keccak-f1600 round constants */
__constant__ uint64_t RC[24] = {
    0x0000000000000001ULL, 0x0000000000008082ULL,
    0x800000000000808AULL, 0x8000000080008000ULL,
    0x000000000000808BULL, 0x0000000080000001ULL,
    0x8000000080008081ULL, 0x8000000000008009ULL,
    0x000000000000008AULL, 0x0000000000000088ULL,
    0x0000000080008009ULL, 0x000000008000000AULL,
    0x000000008000808BULL, 0x800000000000008BULL,
    0x8000000000008089ULL, 0x8000000000008003ULL,
    0x8000000000008002ULL, 0x8000000000000080ULL,
    0x000000000000800AULL, 0x800000008000000AULL,
    0x8000000080008081ULL, 0x8000000000008080ULL,
    0x0000000080000001ULL, 0x8000000080008008ULL,
};

/* Golden ratio fixed-point powers: φ^k × 2^32 */
__constant__ uint64_t PHI_FP[16] = {
    4294967296ULL,     6949403065ULL,     11244370361ULL,    18193773427ULL,
    29438143788ULL,    47631917215ULL,    77070061004ULL,    124701978219ULL,
    201772039223ULL,   326474017443ULL,   528246056666ULL,   854720074109ULL,
    1382966130776ULL,  2237686204885ULL,  3620652335660ULL,  5858338540545ULL,
};

/* AES S-box (FIPS 197) */
__constant__ uint8_t AES_SBOX[256] = {
    0x63,0x7c,0x77,0x7b,0xf2,0x6b,0x6f,0xc5,0x30,0x01,0x67,0x2b,0xfe,0xd7,0xab,0x76,
    0xca,0x82,0xc9,0x7d,0xfa,0x59,0x47,0xf0,0xad,0xd4,0xa2,0xaf,0x9c,0xa4,0x72,0xc0,
    0xb7,0xfd,0x93,0x26,0x36,0x3f,0xf7,0xcc,0x34,0xa5,0xe5,0xf1,0x71,0xd8,0x31,0x15,
    0x04,0xc7,0x23,0xc3,0x18,0x96,0x05,0x9a,0x07,0x12,0x80,0xe2,0xeb,0x27,0xb2,0x75,
    0x09,0x83,0x2c,0x1a,0x1b,0x6e,0x5a,0xa0,0x52,0x3b,0xd6,0xb3,0x29,0xe3,0x2f,0x84,
    0x53,0xd1,0x00,0xed,0x20,0xfc,0xb1,0x5b,0x6a,0xcb,0xbe,0x39,0x4a,0x4c,0x58,0xcf,
    0xd0,0xef,0xaa,0xfb,0x43,0x4d,0x33,0x85,0x45,0xf9,0x02,0x7f,0x50,0x3c,0x9f,0xa8,
    0x51,0xa3,0x40,0x8f,0x92,0x9d,0x38,0xf5,0xbc,0xb6,0xda,0x21,0x10,0xff,0xf3,0xd2,
    0xcd,0x0c,0x13,0xec,0x5f,0x97,0x44,0x17,0xc4,0xa7,0x7e,0x3d,0x64,0x5d,0x19,0x73,
    0x60,0x81,0x4f,0xdc,0x22,0x2a,0x90,0x88,0x46,0xee,0xb8,0x14,0xde,0x5e,0x0b,0xdb,
    0xe0,0x32,0x3a,0x0a,0x49,0x06,0x24,0x5c,0xc2,0xd3,0xac,0x62,0x91,0x95,0xe4,0x79,
    0xe7,0xc8,0x37,0x6d,0x8d,0xd5,0x4e,0xa9,0x6c,0x56,0xf4,0xea,0x65,0x7a,0xae,0x08,
    0xba,0x78,0x25,0x2e,0x1c,0xa6,0xb4,0xc6,0xe8,0xdd,0x74,0x1f,0x4b,0xbd,0x8b,0x8a,
    0x70,0x3e,0xb5,0x66,0x48,0x03,0xf6,0x0e,0x61,0x35,0x57,0xb9,0x86,0xc1,0x1d,0x9e,
    0xe1,0xf8,0x98,0x11,0x69,0xd9,0x8e,0x94,0x9b,0x1e,0x87,0xe9,0xce,0x55,0x28,0xdf,
    0x8c,0xa1,0x89,0x0d,0xbf,0xe6,0x42,0x68,0x41,0x99,0x2d,0x0f,0xb0,0x54,0xbb,0x16,
};

/* AES round constants */
__constant__ uint8_t AES_RCON[10] = {
    0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36
};

/* ========================================================================== */
/* Keccak-f1600                                                                */
/* ========================================================================== */

__device__ void keccak_f1600(uint64_t *st)
{
    uint64_t bc0, bc1, bc2, bc3, bc4, t;

    #pragma unroll 4
    for (int rnd = 0; rnd < 24; rnd++) {
        /* Theta */
        bc0 = st[0]^st[5]^st[10]^st[15]^st[20];
        bc1 = st[1]^st[6]^st[11]^st[16]^st[21];
        bc2 = st[2]^st[7]^st[12]^st[17]^st[22];
        bc3 = st[3]^st[8]^st[13]^st[18]^st[23];
        bc4 = st[4]^st[9]^st[14]^st[19]^st[24];
        t=bc4^ROL64(bc1,1); st[0]^=t;st[5]^=t;st[10]^=t;st[15]^=t;st[20]^=t;
        t=bc0^ROL64(bc2,1); st[1]^=t;st[6]^=t;st[11]^=t;st[16]^=t;st[21]^=t;
        t=bc1^ROL64(bc3,1); st[2]^=t;st[7]^=t;st[12]^=t;st[17]^=t;st[22]^=t;
        t=bc2^ROL64(bc4,1); st[3]^=t;st[8]^=t;st[13]^=t;st[18]^=t;st[23]^=t;
        t=bc3^ROL64(bc0,1); st[4]^=t;st[9]^=t;st[14]^=t;st[19]^=t;st[24]^=t;

        /* Rho+Pi — fully inlined */
        t=st[1];
        bc0=st[10];st[10]=ROL64(t, 1);t=bc0;
        bc0=st[ 7];st[ 7]=ROL64(t, 3);t=bc0;
        bc0=st[11];st[11]=ROL64(t, 6);t=bc0;
        bc0=st[17];st[17]=ROL64(t,10);t=bc0;
        bc0=st[18];st[18]=ROL64(t,15);t=bc0;
        bc0=st[ 3];st[ 3]=ROL64(t,21);t=bc0;
        bc0=st[ 5];st[ 5]=ROL64(t,28);t=bc0;
        bc0=st[16];st[16]=ROL64(t,36);t=bc0;
        bc0=st[ 8];st[ 8]=ROL64(t,45);t=bc0;
        bc0=st[21];st[21]=ROL64(t,55);t=bc0;
        bc0=st[24];st[24]=ROL64(t, 2);t=bc0;
        bc0=st[ 4];st[ 4]=ROL64(t,14);t=bc0;
        bc0=st[15];st[15]=ROL64(t,27);t=bc0;
        bc0=st[23];st[23]=ROL64(t,41);t=bc0;
        bc0=st[19];st[19]=ROL64(t,56);t=bc0;
        bc0=st[13];st[13]=ROL64(t, 8);t=bc0;
        bc0=st[12];st[12]=ROL64(t,25);t=bc0;
        bc0=st[ 2];st[ 2]=ROL64(t,43);t=bc0;
        bc0=st[20];st[20]=ROL64(t,62);t=bc0;
        bc0=st[14];st[14]=ROL64(t,18);t=bc0;
        bc0=st[22];st[22]=ROL64(t,39);t=bc0;
        bc0=st[ 9];st[ 9]=ROL64(t,61);t=bc0;
        bc0=st[ 6];st[ 6]=ROL64(t,20);t=bc0;
                   st[ 1]=ROL64(t,44);

        /* Chi */
        CHI_ROW(0) CHI_ROW(5) CHI_ROW(10) CHI_ROW(15) CHI_ROW(20)

        /* Iota */
        st[0] ^= RC[rnd];
    }
}

/* ========================================================================== */
/* Streaming Keccak absorb / finalize                                          */
/* ========================================================================== */

__device__ void keccak_absorb(uint64_t *st, int *pos, int rate,
                              const uint8_t *in_data, int inlen)
{
    while (inlen > 0) {
        int chunk = rate - *pos;
        if (chunk > inlen) chunk = inlen;
        int off = *pos;
        int i = 0;
        if ((off & 7) == 0) {
            int ulongs = chunk >> 3;
            for (int u = 0; u < ulongs; u++) {
                uint64_t v = 0;
                for (int b = 0; b < 8; b++)
                    v |= (uint64_t)in_data[u * 8 + b] << (b * 8);
                st[(off >> 3) + u] ^= v;
            }
            i = ulongs << 3;
        }
        for (; i < chunk; i++)
            ((uint8_t *)st)[off + i] ^= in_data[i];
        in_data += chunk;
        inlen   -= chunk;
        *pos    += chunk;
        if (*pos == rate) {
            keccak_f1600(st);
            *pos = 0;
        }
    }
}

__device__ void keccak_absorb_global(uint64_t *st, int *pos, int rate,
                                     const uint8_t *in_data, int inlen)
{
    while (inlen > 0) {
        int chunk = rate - *pos;
        if (chunk > inlen) chunk = inlen;
        int off = *pos;
        int i = 0;
        if ((off & 7) == 0) {
            int ulongs = chunk >> 3;
            for (int u = 0; u < ulongs; u++) {
                uint64_t v = 0;
                for (int b = 0; b < 8; b++)
                    v |= (uint64_t)in_data[u * 8 + b] << (b * 8);
                st[(off >> 3) + u] ^= v;
            }
            i = ulongs << 3;
        }
        for (; i < chunk; i++)
            ((uint8_t *)st)[off + i] ^= in_data[i];
        in_data += chunk;
        inlen   -= chunk;
        *pos    += chunk;
        if (*pos == rate) {
            keccak_f1600(st);
            *pos = 0;
        }
    }
}

__device__ void keccak_finalize(uint64_t *st, int pos, int rate,
                                uint8_t pad_byte, uint8_t *out, int outlen)
{
    ((uint8_t *)st)[pos]      ^= pad_byte;
    ((uint8_t *)st)[rate - 1] ^= 0x80;
    keccak_f1600(st);
    int ulongs = outlen >> 3;
    uint64_t *out64 = (uint64_t *)out;
    for (int i = 0; i < ulongs; i++) out64[i] = st[i];
    for (int i = (ulongs << 3); i < outlen; i++)
        out[i] = ((uint8_t *)st)[i];
}

/* ========================================================================== */
/* Hash convenience wrappers                                                   */
/* ========================================================================== */

/* Keccak-256: rate=136, padding=0x01, output=32 B (pre-NIST, Ethereum-style) */
__device__ void keccak256(const uint8_t *in_data, int inlen, uint8_t *out)
{
    uint64_t st[25]; int pos = 0;
    for (int i = 0; i < 25; i++) st[i] = 0;
    keccak_absorb(st, &pos, 136, in_data, inlen);
    keccak_finalize(st, pos, 136, 0x01, out, 32);
}

/*
 * Specialized Keccak-256 for exactly 136-byte input (= Keccak-256 rate).
 * Used in random_read_mix hot loop.
 */
__device__ void keccak256_136_mix(const uint64_t acc64[8], const uint64_t chunk64[8],
                                  uint64_t r_val, uint64_t out64[4])
{
    uint64_t st[25];
    #pragma unroll 25
    for (int i = 0; i < 25; i++) st[i] = 0;

    #pragma unroll 8
    for (int i = 0; i < 8; i++) st[i] = acc64[i];
    #pragma unroll 8
    for (int i = 0; i < 8; i++) st[8 + i] = chunk64[i];
    st[16] = r_val;

    keccak_f1600(st);

    st[0]  ^= 0x01ULL;
    st[16] ^= 0x8000000000000000ULL;
    keccak_f1600(st);

    #pragma unroll 4
    for (int i = 0; i < 4; i++) out64[i] = st[i];
}

/* SHA3-512: rate=72, padding=0x06, output=64 B (NIST SHA-3) */
__device__ void sha3_512(const uint8_t *in_data, int inlen, uint8_t *out)
{
    uint64_t st[25]; int pos = 0;
    for (int i = 0; i < 25; i++) st[i] = 0;
    keccak_absorb(st, &pos, 72, in_data, inlen);
    keccak_finalize(st, pos, 72, 0x06, out, 64);
}

/* ========================================================================== */
/* Step 3: Golden Matrix (64 B → 64 B)                                        */
/* ========================================================================== */

__device__ void golden_matrix(const uint8_t *in64, uint8_t *out64)
{
    uint64_t result[8];
    for (int i = 0; i < 8; i++) {
        uint64_t sum = 0;
        for (int j = 0; j < 8; j++)
            sum += (uint64_t)in64[i * 8 + j] * PHI_FP[i + j];
        result[i] = sum >> 32;
    }
    for (int i = 0; i < 8; i++) {
        uint64_t v = result[i];
        for (int b = 0; b < 8; b++)
            out64[i * 8 + b] = (uint8_t)(v >> (b * 8));
    }
}

/* ========================================================================== */
/* Step 4: Memory-Hard Transform (64 B → 64 B, 64 KiB scratchpad)             */
/* ========================================================================== */

__device__ void init_scratchpad(const uint8_t seed[64], uint8_t *pad)
{
    uint8_t state[64];
    for (int i = 0; i < 64; i++) state[i] = seed[i];

    for (uint32_t blk = 0; blk < BLOCK_COUNT; blk++) {
        uint8_t input[72]; /* state(64) + counter(8) */
        for (int i = 0; i < 64; i++) input[i] = state[i];
        uint64_t counter = (uint64_t)blk;
        for (int b = 0; b < 8; b++) input[64 + b] = (uint8_t)(counter >> (b * 8));

        uint8_t out[64];
        sha3_512(input, 72, out);

        uint32_t off = blk * BLOCK_SIZE;
        for (int i = 0; i < BLOCK_SIZE; i++) {
            pad[off + i] = out[i];
            state[i]     = out[i];
        }
    }
}

__device__ void mix_block(uint8_t *pad, uint32_t index, uint64_t pass, int forward)
{
    uint32_t prev_index;
    if (forward)
        prev_index = (index == 0) ? (BLOCK_COUNT - 1) : (index - 1);
    else
        prev_index = (index + 1 == BLOCK_COUNT) ? 0 : (index + 1);

    uint32_t cur_off  = index * BLOCK_SIZE;
    uint32_t prev_off = prev_index * BLOCK_SIZE;

    uint64_t idx_val = 0;
    for (int b = 0; b < 8; b++)
        idx_val |= (uint64_t)pad[cur_off + b] << (b * 8);
    uint32_t rand_index = (uint32_t)((idx_val ^ pass ^ (uint64_t)index) % BLOCK_COUNT);
    uint32_t rand_off = rand_index * BLOCK_SIZE;

    uint8_t current[BLOCK_SIZE], prev[BLOCK_SIZE], random_blk[BLOCK_SIZE];
    for (int i = 0; i < BLOCK_SIZE; i++) {
        current[i]    = pad[cur_off  + i];
        prev[i]       = pad[prev_off + i];
        random_blk[i] = pad[rand_off + i];
    }

    uint8_t pass_bytes[8], index_bytes[8];
    for (int b = 0; b < 8; b++) pass_bytes[b]  = (uint8_t)(pass >> (b * 8));
    for (int b = 0; b < 8; b++) index_bytes[b] = (uint8_t)((uint64_t)index >> (b * 8));

    uint64_t st[25]; int pos = 0;
    for (int i = 0; i < 25; i++) st[i] = 0;
    keccak_absorb(st, &pos, 72, current,     BLOCK_SIZE);
    keccak_absorb(st, &pos, 72, prev,        BLOCK_SIZE);
    keccak_absorb(st, &pos, 72, random_blk,  BLOCK_SIZE);
    keccak_absorb(st, &pos, 72, pass_bytes,  8);
    keccak_absorb(st, &pos, 72, index_bytes, 8);

    uint8_t mixed[64];
    keccak_finalize(st, pos, 72, 0x06, mixed, 64);

    for (int i = 0; i < BLOCK_SIZE; i++)
        pad[cur_off + i] ^= mixed[i];
}

__device__ void sequential_passes(uint8_t *pad)
{
    for (int pass = 0; pass < PASSES; pass++) {
        int forward = (pass % 2 == 0);
        if (forward) {
            for (uint32_t i = 0; i < BLOCK_COUNT; i++)
                mix_block(pad, i, (uint64_t)pass, 1);
        } else {
            for (int i = BLOCK_COUNT - 1; i >= 0; i--)
                mix_block(pad, (uint32_t)i, (uint64_t)pass, 0);
        }
    }
}

__device__ void random_read_mix(const uint8_t seed[64], const uint8_t *pad,
                                uint8_t out[64])
{
    uint8_t acc[64];
    { uint64_t *d = (uint64_t *)acc; const uint64_t *s = (const uint64_t *)seed;
      for (int i = 0; i < 8; i++) d[i] = s[i]; }

    uint64_t pos_val = *(const uint64_t *)seed;
    uint32_t pos = (uint32_t)(pos_val % BLOCK_COUNT);

    for (int r = 0; r < RANDOM_READS; r++) {
        uint32_t off = pos * BLOCK_SIZE;

        const uint64_t *gsrc = (const uint64_t *)(pad + off);
        uint64_t chunk64[8];
        #pragma unroll 8
        for (int i = 0; i < 8; i++) chunk64[i] = gsrc[i];

        uint64_t d64[4];
        keccak256_136_mix((const uint64_t *)acc, chunk64, (uint64_t)r, d64);

        {
            uint64_t *a64 = (uint64_t *)acc;
            #pragma unroll 4
            for (int u = 0; u < 4; u++) a64[u] ^= d64[u];
        }
        {
            const uint8_t *d8 = (const uint8_t *)d64;
            for (int i = 0; i < 32; i++)
                acc[32 + i] = (uint8_t)((uint32_t)acc[32 + i] + (uint32_t)d8[i]);
        }

        pos = (uint32_t)((d64[0] ^ (uint64_t)pos ^ (uint64_t)r) % BLOCK_COUNT);
    }

    /* Final hash: SHA3-512(acc || pad[0:64] || pad[last_64:]) */
    uint8_t first_blk[BLOCK_SIZE], last_blk[BLOCK_SIZE];
    {
        const uint64_t *fp = (const uint64_t *)pad;
        const uint64_t *lp = (const uint64_t *)(pad + SCRATCHPAD_SIZE - BLOCK_SIZE);
        uint64_t *fd = (uint64_t *)first_blk;
        uint64_t *ld = (uint64_t *)last_blk;
        for (int i = 0; i < 8; i++) {
            fd[i] = fp[i];
            ld[i] = lp[i];
        }
    }

    uint64_t fst[25]; int fpos = 0;
    for (int i = 0; i < 25; i++) fst[i] = 0;
    keccak_absorb(fst, &fpos, 72, acc,       64);
    keccak_absorb(fst, &fpos, 72, first_blk, BLOCK_SIZE);
    keccak_absorb(fst, &fpos, 72, last_blk,  BLOCK_SIZE);
    keccak_finalize(fst, fpos, 72, 0x06, out, 64);
}

__device__ void memory_hard_transform(const uint8_t input[64], uint8_t *pad,
                                      uint8_t output[64])
{
    init_scratchpad(input, pad);
    sequential_passes(pad);
    random_read_mix(input, pad, output);
}

/* ========================================================================== */
/* BLAKE3 Engine — for Ekam Deeksha variant                                    */
/* ========================================================================== */

__constant__ uint32_t BLAKE3_IV[8] = {
    0x6A09E667u, 0xBB67AE85u, 0x3C6EF372u, 0xA54FF53Au,
    0x510E527Fu, 0x9B05688Cu, 0x1F83D9ABu, 0x5BE0CD19u
};

__constant__ uint8_t BLAKE3_MSG_PERM[16] = {
    2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8
};

#define BLAKE3_CHUNK_START 1u
#define BLAKE3_CHUNK_END   2u
#define BLAKE3_ROOT        8u

__device__ __forceinline__ uint32_t b3_rotr32(uint32_t x, int n) {
    return (x >> n) | (x << (32 - n));
}

__device__ void b3_g(uint32_t *st, int a, int b, int c, int d, uint32_t mx, uint32_t my) {
    st[a] = st[a] + st[b] + mx;
    st[d] = b3_rotr32(st[d] ^ st[a], 16);
    st[c] = st[c] + st[d];
    st[b] = b3_rotr32(st[b] ^ st[c], 12);
    st[a] = st[a] + st[b] + my;
    st[d] = b3_rotr32(st[d] ^ st[a], 8);
    st[c] = st[c] + st[d];
    st[b] = b3_rotr32(st[b] ^ st[c], 7);
}

__device__ void b3_round(uint32_t *st, const uint32_t *msg) {
    b3_g(st, 0, 4,  8, 12, msg[0],  msg[1]);
    b3_g(st, 1, 5,  9, 13, msg[2],  msg[3]);
    b3_g(st, 2, 6, 10, 14, msg[4],  msg[5]);
    b3_g(st, 3, 7, 11, 15, msg[6],  msg[7]);
    b3_g(st, 0, 5, 10, 15, msg[8],  msg[9]);
    b3_g(st, 1, 6, 11, 12, msg[10], msg[11]);
    b3_g(st, 2, 7,  8, 13, msg[12], msg[13]);
    b3_g(st, 3, 4,  9, 14, msg[14], msg[15]);
}

__device__ void b3_permute(uint32_t msg[16]) {
    uint32_t tmp[16];
    for (int i = 0; i < 16; i++) tmp[i] = msg[BLAKE3_MSG_PERM[i]];
    for (int i = 0; i < 16; i++) msg[i] = tmp[i];
}

__device__ void b3_compress(const uint32_t cv[8], const uint32_t bw[16],
                            uint64_t counter, uint32_t block_len, uint32_t flags,
                            uint32_t output[16])
{
    uint32_t st[16] = {
        cv[0], cv[1], cv[2], cv[3],
        cv[4], cv[5], cv[6], cv[7],
        BLAKE3_IV[0], BLAKE3_IV[1], BLAKE3_IV[2], BLAKE3_IV[3],
        (uint32_t)(counter & 0xFFFFFFFFu),
        (uint32_t)(counter >> 32),
        block_len,
        flags
    };
    uint32_t msg[16];
    for (int i = 0; i < 16; i++) msg[i] = bw[i];
    #pragma unroll 6
    for (int i = 0; i < 6; i++) {
        b3_round(st, msg);
        b3_permute(msg);
    }
    b3_round(st, msg);

    for (int i = 0; i < 8; i++) {
        st[i] ^= st[i + 8];
        st[i + 8] ^= cv[i];
    }

    for (int i = 0; i < 16; i++) output[i] = st[i];
}

__device__ void b3_compress_cv(const uint32_t cv[8], const uint32_t bw[16],
                               uint64_t counter, uint32_t block_len, uint32_t flags,
                               uint32_t out_cv[8])
{
    uint32_t full[16];
    b3_compress(cv, bw, counter, block_len, flags, full);
    for (int i = 0; i < 8; i++) out_cv[i] = full[i];
}

__device__ void b3_load_words(const uint8_t *buf, int len, uint32_t words[16]) {
    for (int i = 0; i < 16; i++) words[i] = 0;
    for (int i = 0; i < len; i++)
        words[i / 4] |= (uint32_t)buf[i] << ((i % 4) * 8);
}

__device__ void b3_load_words_global(const uint8_t *buf, int len, uint32_t words[16]) {
    const uint32_t *buf32 = (const uint32_t *)buf;
    int wcount = len >> 2;
    for (int i = 0; i < wcount; i++) words[i] = buf32[i];
    for (int i = wcount; i < 16; i++) words[i] = 0;
    int done = wcount << 2;
    if (done < len) {
        uint32_t w = 0;
        for (int i = done; i < len; i++)
            w |= (uint32_t)buf[i] << ((i - done) * 8);
        words[wcount] = w;
    }
}

typedef struct {
    uint32_t input_cv[8];
    uint32_t block_words[16];
    uint32_t block_len;
    uint32_t flags;
} B3ChunkOut;

__device__ B3ChunkOut b3_hash_single_chunk(const uint8_t *input, uint32_t input_len) {
    B3ChunkOut out;
    uint32_t cv[8];
    for (int i = 0; i < 8; i++) cv[i] = BLAKE3_IV[i];
    uint32_t offset = 0;
    while (offset < input_len) {
        uint32_t remaining = input_len - offset;
        uint32_t this_len = (remaining > 64u) ? 64u : remaining;
        int is_first = (offset == 0);
        int is_last  = (offset + this_len >= input_len);
        uint32_t fl = 0u;
        if (is_first) fl |= BLAKE3_CHUNK_START;
        if (is_last)  fl |= BLAKE3_CHUNK_END;
        uint32_t bw[16];
        b3_load_words(input + offset, (int)this_len, bw);
        if (is_last) {
            for (int i = 0; i < 8; i++) out.input_cv[i] = cv[i];
            for (int i = 0; i < 16; i++) out.block_words[i] = bw[i];
            out.block_len = this_len;
            out.flags = fl;
            return out;
        }
        b3_compress_cv(cv, bw, 0ULL, this_len, fl, cv);
        offset += this_len;
    }
    for (int i = 0; i < 8; i++) out.input_cv[i] = BLAKE3_IV[i];
    for (int i = 0; i < 16; i++) out.block_words[i] = 0;
    out.block_len = 0;
    out.flags = BLAKE3_CHUNK_START | BLAKE3_CHUNK_END;
    return out;
}

__device__ void b3_xof_fill_global(B3ChunkOut co, uint8_t *buf, uint32_t buf_len) {
    uint32_t *buf32 = (uint32_t *)buf;
    uint32_t ob = 0, written = 0;
    while (written < buf_len) {
        uint32_t st[16];
        b3_compress(co.input_cv, co.block_words, (uint64_t)ob,
                    co.block_len, co.flags | BLAKE3_ROOT, st);
        uint32_t to_write = min(64u, buf_len - written);
        uint32_t words = to_write >> 2;
        uint32_t base = written >> 2;
        for (uint32_t i = 0; i < words; i++)
            buf32[base + i] = st[i];
        uint32_t done = words << 2;
        for (uint32_t i = done; i < to_write; i++)
            buf[written + i] = (uint8_t)(st[i / 4] >> ((i % 4) * 8));
        written += to_write;
        ob++;
    }
}

__device__ void b3_xof_fill_private(B3ChunkOut co, uint8_t *buf, uint32_t buf_len) {
    uint32_t ob = 0, written = 0;
    while (written < buf_len) {
        uint32_t st[16];
        b3_compress(co.input_cv, co.block_words, (uint64_t)ob,
                    co.block_len, co.flags | BLAKE3_ROOT, st);
        uint32_t to_write = min(64u, buf_len - written);
        uint32_t full_words = to_write >> 2;
        uint32_t *dst32 = (uint32_t *)(buf + written);
        for (uint32_t w = 0; w < full_words; w++) dst32[w] = st[w];
        for (uint32_t i = (full_words << 2); i < to_write; i++)
            buf[written + i] = (uint8_t)(st[i / 4] >> ((i % 4) * 8));
        written += to_write;
        ob++;
    }
}

/* ========================================================================== */
/* Ekam Deeksha Scratchpad — Blake3 XOF                                        */
/* ========================================================================== */

__constant__ uint8_t EKAM_DOMAIN_SEP[23] = {
    'E','K','A','M','_','S','C','R','A','T','C','H','P','A','D','_','I','N','I','T','_','V','1'
};

__device__ void ekam_init_scratchpad(const uint8_t seed[64], uint8_t *pad)
{
    uint8_t input[87];
    for (int i = 0; i < 64; i++) input[i] = seed[i];
    for (int i = 0; i < 23; i++) input[64 + i] = EKAM_DOMAIN_SEP[i];
    B3ChunkOut co = b3_hash_single_chunk(input, 87u);
    b3_xof_fill_global(co, pad, SCRATCHPAD_SIZE);
}

__device__ void ekam_mix_block(uint8_t *pad, uint32_t index, uint64_t pass, int forward)
{
    uint32_t prev_index;
    if (forward)
        prev_index = (index == 0) ? (BLOCK_COUNT - 1) : (index - 1);
    else
        prev_index = (index + 1 == BLOCK_COUNT) ? 0 : (index + 1);

    uint32_t cur_off  = index * BLOCK_SIZE;
    uint32_t prev_off = prev_index * BLOCK_SIZE;

    uint64_t idx_val = *((const uint64_t *)(pad + cur_off));
    uint32_t rand_index = (uint32_t)((idx_val ^ pass ^ (uint64_t)index) % BLOCK_COUNT);
    uint32_t rand_off = rand_index * BLOCK_SIZE;

    uint32_t cv[8];
    for (int i = 0; i < 8; i++) cv[i] = BLAKE3_IV[i];
    uint32_t bw[16];

    /* Block 0: cur[0..64], CHUNK_START */
    b3_load_words_global(pad + cur_off, 64, bw);
    b3_compress_cv(cv, bw, 0ULL, 64, BLAKE3_CHUNK_START, cv);

    /* Block 1: prev[0..64] */
    b3_load_words_global(pad + prev_off, 64, bw);
    b3_compress_cv(cv, bw, 0ULL, 64, 0, cv);

    /* Block 2: rand[0..64] */
    b3_load_words_global(pad + rand_off, 64, bw);
    b3_compress_cv(cv, bw, 0ULL, 64, 0, cv);

    /* Block 3: pass(8) || idx(8) = 16 bytes, CHUNK_END */
    for (int i = 0; i < 16; i++) bw[i] = 0;
    bw[0] = (uint32_t)(pass & 0xFFFFFFFFu);
    bw[1] = (uint32_t)(pass >> 32);
    bw[2] = (uint32_t)((uint64_t)index & 0xFFFFFFFFu);
    bw[3] = (uint32_t)((uint64_t)index >> 32);

    B3ChunkOut co;
    for (int i = 0; i < 8; i++) co.input_cv[i] = cv[i];
    for (int i = 0; i < 16; i++) co.block_words[i] = bw[i];
    co.block_len = 16;
    co.flags = BLAKE3_CHUNK_END;

    uint8_t mixed[64];
    b3_xof_fill_private(co, mixed, 64u);

    uint64_t *dst = (uint64_t *)(pad + cur_off);
    uint64_t *src = (uint64_t *)mixed;
    for (int i = 0; i < 8; i++)
        dst[i] ^= src[i];
}

__device__ void ekam_sequential_passes(uint8_t *pad)
{
    for (int pass = 0; pass < PASSES; pass++) {
        int forward = (pass % 2 == 0);
        if (forward) {
            for (uint32_t i = 0; i < BLOCK_COUNT; i++)
                ekam_mix_block(pad, i, (uint64_t)pass, 1);
        } else {
            for (int i = BLOCK_COUNT - 1; i >= 0; i--)
                ekam_mix_block(pad, (uint32_t)i, (uint64_t)pass, 0);
        }
    }
}

__device__ void ekam_memory_hard_transform(const uint8_t input[64], uint8_t *pad,
                                           uint8_t output[64])
{
    ekam_init_scratchpad(input, pad);
    ekam_sequential_passes(pad);
    random_read_mix(input, pad, output);
}

/* ========================================================================== */
/* Step 5: NPU Mix — Variable-topology INT8 MLP with LayerNorm + GELU + Residual */
/*                                                                             */
/* Topologies (rotate per epoch % 4):                                          */
/*   0: Standard   64→128→64   (2 layers)                                     */
/*   1: ThreeLayer 64→96→128→64 (3 layers)                                    */
/*   2: Wide       64→256→64   (2 layers)                                     */
/*   3: Deep       64→64→64→64 (3 layers)                                     */
/*                                                                             */
/* Packed buffers layout:                                                      */
/*   weights: [layer0_weights..., layer1_weights..., ...]                      */
/*   biases:  [layer0_bias..., layer1_bias..., ...]                            */
/*   scales:  [layer0_scale..., layer1_scale..., ...]                          */
/*   meta:    [num_layers, in0, out0, in1, out1, ...]                          */
/* ========================================================================== */

__device__ int gelu_int8(int x)
{
    int num = x * (128 + x);
    int result = num >> 8;
    return max(-128, min(127, result));
}

__device__ void npu_mix_packed(const uint8_t in64[64], uint8_t out64[64],
                               const int8_t   *weights,   /* all layers concatenated */
                               const int8_t   *biases,    /* all layers concatenated */
                               const int16_t  *scales,    /* all layers concatenated */
                               const uint32_t *meta)      /* [num_layers, in0, out0, ...] */
{
    int current[256];   /* max hidden dim = 256 (Wide topology) */
    int next[256];

    /* Convert input u8 → i32 (signed reinterpret: (int8_t)in64[i]) */
    for (int i = 0; i < 64; i++)
        current[i] = (int)((int8_t)in64[i]);

    /* Save residual (input dim is always 64) */
    int residual[64];
    for (int i = 0; i < 64; i++)
        residual[i] = current[i];

    int num_layers = (int)meta[0];
    int w_off = 0;    /* weight offset */
    int b_off = 0;    /* bias offset */
    int s_off = 0;    /* scale offset */
    int cur_dim = 64;

    for (int layer = 0; layer < num_layers; layer++) {
        int in_dim  = (int)meta[1 + 2 * layer];
        int out_dim = (int)meta[2 + 2 * layer];

        /* MatMul + bias */
        for (int i = 0; i < out_dim; i++) {
            int acc = (int)biases[b_off + i] * 32;
            for (int j = 0; j < in_dim; j++)
                acc += current[j] * (int)weights[w_off + i * in_dim + j];
            next[i] = max(-128, min(127, acc >> 12));
        }

        /* LayerNorm */
        {
            int64_t sum = 0;
            for (int i = 0; i < out_dim; i++) sum += (int64_t)next[i];
            int mean = (int)(sum / (int64_t)out_dim);
            int64_t var_sum = 0;
            for (int i = 0; i < out_dim; i++) {
                int64_t d = (int64_t)(next[i] - mean);
                var_sum += d * d;
            }
            int std_approx = (int)sqrtf((float)(var_sum / (int64_t)out_dim)) + 1;
            for (int i = 0; i < out_dim; i++) {
                int normalized = ((next[i] - mean) * 128) / std_approx;
                next[i] = max(-128, min(127, (normalized * (int)scales[s_off + i]) >> 8));
            }
        }

        /* GELU for all but last layer */
        if (layer < num_layers - 1) {
            for (int i = 0; i < out_dim; i++)
                next[i] = gelu_int8(next[i]);
        }

        /* Advance: next → current */
        for (int i = 0; i < out_dim; i++)
            current[i] = next[i];
        cur_dim = out_dim;

        /* Advance offsets */
        w_off += in_dim * out_dim;
        b_off += out_dim;
        s_off += out_dim;
    }

    /* Residual add + output conversion (final dim is always 64) */
    for (int i = 0; i < 64; i++) {
        int v = max(-128, min(127, current[i] + residual[i]));
        out64[i] = (uint8_t)v;
    }
}

/* ========================================================================== */
/* AES-128 single-block encryption (FIPS 197)                                  */
/* ========================================================================== */

__device__ void aes_shift_rows(uint8_t s[16])
{
    uint8_t t;
    t = s[1]; s[1] = s[5]; s[5] = s[9]; s[9] = s[13]; s[13] = t;
    t = s[2]; s[2] = s[10]; s[10] = t;
    t = s[6]; s[6] = s[14]; s[14] = t;
    t = s[15]; s[15] = s[11]; s[11] = s[7]; s[7] = s[3]; s[3] = t;
}

__device__ void aes_mix_columns(uint8_t s[16])
{
    for (int c = 0; c < 4; c++) {
        int off = c * 4;
        uint8_t a0 = s[off], a1 = s[off+1], a2 = s[off+2], a3 = s[off+3];
        s[off]   = XTIME(a0) ^ XTIME(a1) ^ a1 ^ a2 ^ a3;
        s[off+1] = a0 ^ XTIME(a1) ^ XTIME(a2) ^ a2 ^ a3;
        s[off+2] = a0 ^ a1 ^ XTIME(a2) ^ XTIME(a3) ^ a3;
        s[off+3] = XTIME(a0) ^ a0 ^ a1 ^ a2 ^ XTIME(a3);
    }
}

__device__ void aes128_encrypt(const uint8_t key[16], uint8_t block[16])
{
    uint8_t rk[176];
    for (int i = 0; i < 16; i++) rk[i] = key[i];

    for (int i = 16; i < 176; i += 4) {
        uint8_t t0 = rk[i-4], t1 = rk[i-3], t2 = rk[i-2], t3 = rk[i-1];
        if ((i & 15) == 0) {
            uint8_t tmp = t0;
            t0 = AES_SBOX[t1] ^ AES_RCON[i/16 - 1];
            t1 = AES_SBOX[t2];
            t2 = AES_SBOX[t3];
            t3 = AES_SBOX[tmp];
        }
        rk[i]   = rk[i-16] ^ t0;
        rk[i+1] = rk[i-15] ^ t1;
        rk[i+2] = rk[i-14] ^ t2;
        rk[i+3] = rk[i-13] ^ t3;
    }

    for (int i = 0; i < 16; i++) block[i] ^= rk[i];

    for (int round = 1; round <= 9; round++) {
        for (int i = 0; i < 16; i++) block[i] = AES_SBOX[block[i]];
        aes_shift_rows(block);
        aes_mix_columns(block);
        int off = round * 16;
        for (int i = 0; i < 16; i++) block[i] ^= rk[off + i];
    }

    for (int i = 0; i < 16; i++) block[i] = AES_SBOX[block[i]];
    aes_shift_rows(block);
    for (int i = 0; i < 16; i++) block[i] ^= rk[160 + i];
}

/* ========================================================================== */
/* Step 6: Cosmic Fusion (64 B → 32 B)                                        */
/* ========================================================================== */

__device__ void fusion_round(uint8_t state[64], uint8_t round_num)
{
    uint8_t hash_input[33];
    {
        uint64_t *hi64 = (uint64_t *)hash_input;
        uint64_t *st64 = (uint64_t *)state;
        for (int i = 0; i < 4; i++) hi64[i] = st64[i];
    }
    hash_input[32] = round_num;

    uint8_t intermediate[32];
    keccak256(hash_input, 33, intermediate);

    uint8_t aes_key[16], block0[16], block1[16];
    {
        uint64_t *k64 = (uint64_t *)aes_key;
        uint64_t *i64 = (uint64_t *)intermediate;
        k64[0] = i64[0]; k64[1] = i64[1];
    }
    {
        uint64_t *b64 = (uint64_t *)block0;
        uint64_t *s64 = (uint64_t *)(state + 32);
        b64[0] = s64[0]; b64[1] = s64[1];
    }
    aes128_encrypt(aes_key, block0);

    uint8_t key2[16];
    {
        uint64_t *k264 = (uint64_t *)key2;
        uint64_t *k64  = (uint64_t *)aes_key;
        k264[0] = k64[0]; k264[1] = k64[1];
    }
    key2[0]  ^= round_num;
    key2[15] ^= 0xAB;
    {
        uint64_t *b64 = (uint64_t *)block1;
        uint64_t *s64 = (uint64_t *)(state + 48);
        b64[0] = s64[0]; b64[1] = s64[1];
    }
    aes128_encrypt(key2, block1);

    uint8_t mask[32];
    {
        uint64_t *m64 = (uint64_t *)mask;
        uint64_t *b064 = (uint64_t *)block0;
        uint64_t *b164 = (uint64_t *)block1;
        m64[0] = b064[0]; m64[1] = b064[1];
        m64[2] = b164[0]; m64[3] = b164[1];
    }

    /* state[32..64] ^= intermediate (evolve upper half FIRST) */
    {
        uint64_t *s64 = (uint64_t *)(state + 32);
        uint64_t *i64 = (uint64_t *)intermediate;
        for (int i = 0; i < 4; i++) s64[i] ^= i64[i];
    }

    /* state[0..32] = intermediate ^ mask (overwrite lower half) */
    {
        uint64_t *s64 = (uint64_t *)state;
        uint64_t *i64 = (uint64_t *)intermediate;
        uint64_t *m64 = (uint64_t *)mask;
        for (int i = 0; i < 4; i++) s64[i] = i64[i] ^ m64[i];
    }
}

__device__ void cosmic_fusion(const uint8_t in64[64], uint8_t hash32[32])
{
    uint8_t state[64];
    { uint64_t *d = (uint64_t *)state; const uint64_t *s = (const uint64_t *)in64;
      for (int i = 0; i < 8; i++) d[i] = s[i]; }

    fusion_round(state, 0);
    fusion_round(state, 1);
    fusion_round(state, 2);
    fusion_round(state, 3);

    uint8_t full[64];
    sha3_512(state, 32, full);
    { uint64_t *d = (uint64_t *)hash32; uint64_t *s = (uint64_t *)full;
      for (int i = 0; i < 4; i++) d[i] = s[i]; }
}

/* Ekam Cosmic Fusion: 8 rounds */
__device__ void cosmic_fusion_ekam(const uint8_t in64[64], uint8_t hash32[32])
{
    uint8_t state[64];
    { uint64_t *d = (uint64_t *)state; const uint64_t *s = (const uint64_t *)in64;
      for (int i = 0; i < 8; i++) d[i] = s[i]; }

    for (uint8_t r = 0; r < 8; r++)
        fusion_round(state, r);

    uint8_t full[64];
    sha3_512(state, 32, full);
    { uint64_t *d = (uint64_t *)hash32; uint64_t *s = (uint64_t *)full;
      for (int i = 0; i < 4; i++) d[i] = s[i]; }
}

/* ========================================================================== */
/* Main Kernel: deeksha_mine                                                   */
/* ========================================================================== */

extern "C" __global__ void deeksha_mine(
    const uint8_t  *__restrict__ header,          /* block header bytes         */
    uint32_t                     header_len,      /* actual header length       */
    uint64_t                     nonce_base,      /* starting nonce             */
    uint8_t        *__restrict__ scratchpad_pool, /* N × 262144 bytes           */
    uint32_t                     target_u32,      /* LE u32 target              */
    uint64_t       *__restrict__ result_nonce,    /* output: winning nonce      */
    uint8_t        *__restrict__ result_hash,     /* output: 32-byte hash       */
    const int8_t   *__restrict__ npu_weights,     /* packed MLP weights         */
    const int8_t   *__restrict__ npu_biases,      /* packed MLP biases          */
    const int16_t  *__restrict__ npu_scales,      /* packed MLP scales          */
    const uint32_t *__restrict__ npu_meta         /* [num_layers, in0, out0...] */
)
{
    uint32_t tid = blockIdx.x * blockDim.x + threadIdx.x;
    uint64_t nonce = nonce_base + (uint64_t)tid;

    /* Per-thread scratchpad in global memory */
    uint8_t *pad = scratchpad_pool + (uint64_t)tid * SCRATCHPAD_SIZE;

    /* Build input: header (≤80 B, zero-padded) + nonce (8 B LE) = 88 B */
    uint8_t input[88];
    for (int i = 0; i < 88; i++) input[i] = 0;
    uint32_t hlen = min(header_len, 80u);
    for (uint32_t i = 0; i < hlen; i++) input[i] = header[i];
    for (int b = 0; b < 8; b++) input[80 + b] = (uint8_t)(nonce >> (b * 8));

    /* Step 1: Keccak-256 (88 B → 32 B) */
    uint8_t s1[32];
    keccak256(input, 88, s1);

    /* Step 2: SHA3-512 (32 B → 64 B) */
    uint8_t s2[64];
    sha3_512(s1, 32, s2);

    /* Step 3: Golden Matrix (64 B → 64 B) */
    uint8_t s3[64];
    golden_matrix(s2, s3);

    /* Step 4: Memory-Hard (64 B → 64 B, 64 KiB scratchpad) */
    uint8_t s4[64];
    memory_hard_transform(s3, pad, s4);

    /* Step 5: NPU Mix (64 B → 64 B) */
    uint8_t s5[64];
    npu_mix_packed(s4, s5, npu_weights, npu_biases, npu_scales, npu_meta);

    /* Step 6: Cosmic Fusion (64 B → 32 B) */
    uint8_t hash[32];
    cosmic_fusion(s5, hash);

    /* Target check: LE u32 from first 4 bytes ≤ target */
    uint32_t state0 = (uint32_t)hash[0]
                    | ((uint32_t)hash[1] <<  8)
                    | ((uint32_t)hash[2] << 16)
                    | ((uint32_t)hash[3] << 24);

    if (state0 <= target_u32) {
        unsigned long long int old = atomicCAS(
            (unsigned long long int *)result_nonce,
            0xFFFFFFFFFFFFFFFFULL, (unsigned long long int)nonce);
        if (old == 0xFFFFFFFFFFFFFFFFULL) {
            for (int i = 0; i < 32; i++)
                result_hash[i] = hash[i];
        }
    }
}

/* ========================================================================== */
/* Ekam Deeksha Mining Kernel                                                  */
/* Steps 1-3: same. Step 4: Blake3 XOF scratchpad. Step 6: 8-round fusion.    */
/* ========================================================================== */

extern "C" __global__ void ekam_deeksha_mine(
    const uint8_t  *__restrict__ header,
    uint32_t                     header_len,
    uint64_t                     nonce_base,
    uint32_t                     nonce_count,
    uint8_t        *__restrict__ scratchpad_pool,
    uint32_t                     target_u32,
    uint64_t       *__restrict__ result_nonce,
    uint8_t        *__restrict__ result_hash,
    const int8_t   *__restrict__ npu_weights,
    const int8_t   *__restrict__ npu_biases,
    const int16_t  *__restrict__ npu_scales,
    const uint32_t *__restrict__ npu_meta
)
{
    uint32_t tid = blockIdx.x * blockDim.x + threadIdx.x;
    if (tid >= nonce_count) return;
    uint64_t nonce = nonce_base + (uint64_t)tid;
    uint8_t *pad = scratchpad_pool + (uint64_t)tid * SCRATCHPAD_SIZE;

    /* Build input: header (<=80 B) + nonce (8 B LE) = 88 B, zero-padded */
    uint8_t input[88];
    uint64_t *inp64 = (uint64_t *)input;
    for (int i = 0; i < 11; i++) inp64[i] = 0;
    uint32_t hlen = min(header_len, 80u);
    const uint32_t *hdr32 = (const uint32_t *)header;
    uint32_t *inp32 = (uint32_t *)input;
    uint32_t hwords = hlen >> 2;
    for (uint32_t i = 0; i < hwords; i++) inp32[i] = hdr32[i];
    for (uint32_t i = (hwords << 2); i < hlen; i++) input[i] = header[i];
    inp64[10] = nonce;

    uint8_t s1[32];
    keccak256(input, 88, s1);

    uint8_t s2[64];
    sha3_512(s1, 32, s2);

    uint8_t s3[64];
    golden_matrix(s2, s3);

    uint8_t s4[64];
    ekam_memory_hard_transform(s3, pad, s4);

    uint8_t s5[64];
    npu_mix_packed(s4, s5, npu_weights, npu_biases, npu_scales, npu_meta);

    uint8_t hash[32];
    cosmic_fusion_ekam(s5, hash);

    /* Target check */
    uint32_t state0 = *(uint32_t *)hash;

    if (state0 <= target_u32) {
        unsigned long long int old = atomicCAS(
            (unsigned long long int *)result_nonce,
            0xFFFFFFFFFFFFFFFFULL, (unsigned long long int)nonce);
        if (old == 0xFFFFFFFFFFFFFFFFULL) {
            uint32_t *rh32 = (uint32_t *)result_hash;
            uint32_t *h32 = (uint32_t *)hash;
            for (int i = 0; i < 8; i++)
                rh32[i] = h32[i];
        }
    }
}

/* ========================================================================== */
/* Debug kernel — outputs per-stage hashes for verification                    */
/* ========================================================================== */

extern "C" __global__ void ekam_deeksha_debug(
    const uint8_t  *__restrict__ header,
    uint32_t                     header_len,
    uint64_t                     nonce,
    uint8_t        *__restrict__ scratchpad_pool,
    uint8_t        *__restrict__ stage_out,
    const int8_t   *__restrict__ npu_weights,
    const int8_t   *__restrict__ npu_biases,
    const int16_t  *__restrict__ npu_scales,
    const uint32_t *__restrict__ npu_meta
)
{
    if (blockIdx.x * blockDim.x + threadIdx.x != 0) return;

    uint8_t *pad = scratchpad_pool;

    uint8_t input[88];
    uint64_t *inp64 = (uint64_t *)input;
    for (int i = 0; i < 11; i++) inp64[i] = 0;
    uint32_t hlen = min(header_len, 80u);
    const uint32_t *hdr32 = (const uint32_t *)header;
    uint32_t *inp32 = (uint32_t *)input;
    uint32_t hwords = hlen >> 2;
    for (uint32_t i = 0; i < hwords; i++) inp32[i] = hdr32[i];
    for (uint32_t i = (hwords << 2); i < hlen; i++) input[i] = header[i];
    inp64[10] = nonce;

    uint8_t s1[32];
    keccak256(input, 88, s1);

    uint8_t s2[64];
    sha3_512(s1, 32, s2);

    uint8_t s3[64];
    golden_matrix(s2, s3);

    uint8_t s4[64];
    ekam_memory_hard_transform(s3, pad, s4);

    uint8_t s5[64];
    npu_mix_packed(s4, s5, npu_weights, npu_biases, npu_scales, npu_meta);

    uint8_t hash[32];
    cosmic_fusion_ekam(s5, hash);

    for (int i = 0; i < 32; i++) stage_out[i] = s1[i];
    for (int i = 0; i < 64; i++) stage_out[32 + i] = s2[i];
    for (int i = 0; i < 64; i++) stage_out[96 + i] = s3[i];
    for (int i = 0; i < 64; i++) stage_out[160 + i] = s4[i];
    for (int i = 0; i < 64; i++) stage_out[224 + i] = s5[i];
    for (int i = 0; i < 32; i++) stage_out[288 + i] = hash[i];
}
