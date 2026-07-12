/*
 * ============================================================================
 *  ZION Native kHeavyHash Library
 *  Full, correct kHeavyHash implementation for Kaspa (KAS) mining.
 *
 *  Algorithm (matches rusty-kaspa / kaspa-hashes reference):
 *    1. PowHash = cSHAKE256("ProofOfWorkHash")(
 *           pre_pow_hash || timestamp_le || 32 zero bytes || nonce_le) -> 32 B
 *    2. Matrix step:
 *       a. Expand PowHash to 64 nibbles (high nibble, then low nibble).
 *       b. Multiply by the fixed 64x64 matrix of 4-bit values (0..15).
 *          The matrix is generated from SHA3-256("KHeavyHash") seed via
 *          XoShiRo256++, retrying until the matrix has full rank (64).
 *       c. For each output row i (0..32):
 *            sum1 = sum_j matrix[2*i  ][j] * vec[j]
 *            sum2 = sum_j matrix[2*i+1][j] * vec[j]
 *            product[i] = ((sum1 >> 10) << 4) | (sum2 >> 10)
 *       d. XOR product with the original PowHash.
 *    3. HeavyHash = cSHAKE256("HeavyHash")(product) -> 32 B
 *
 *  Compilation:
 *    macOS: clang -O3 -fPIC -shared -std=c11 -o libkheavyhash_zion.dylib kheavyhash_native.c
 *    Linux: gcc -O3 -fPIC -shared -std=c11 -o libkheavyhash_zion.so kheavyhash_native.c
 * ============================================================================
 */

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

/* kHeavyHash constants */
#define KHEAVY_MATRIX_SIZE 64
#define KHEAVY_HASH_SIZE   32
#define KECCAK_RATE        136   /* 1088 bits for SHA3-256 / cSHAKE256 */

/* Keccak-f[1600] round constants */
static const uint64_t KECCAK_RC[24] = {
    0x0000000000000001ULL, 0x0000000000008082ULL, 0x800000000000808aULL,
    0x8000000080008000ULL, 0x000000000000808bULL, 0x0000000080000001ULL,
    0x8000000080008081ULL, 0x8000000000008009ULL, 0x000000000000008aULL,
    0x0000000000000088ULL, 0x0000000080008009ULL, 0x000000008000000aULL,
    0x000000008000808bULL, 0x800000000000008bULL, 0x8000000000008089ULL,
    0x8000000000008003ULL, 0x8000000000008002ULL, 0x8000000000000080ULL,
    0x000000000000800aULL, 0x800000008000000aULL, 0x8000000080008081ULL,
    0x8000000000008080ULL, 0x0000000080000001ULL, 0x8000000080008008ULL,
};

static inline uint64_t rotl64(uint64_t x, int n) {
    return (x << n) | (x >> (64 - n));
}

/* Keccak-f[1600] permutation */
static void keccak_f1600(uint64_t state[25]) {
    for (int round = 0; round < 24; round++) {
        /* Theta */
        uint64_t c[5], d[5];
        for (int x = 0; x < 5; x++) {
            c[x] = state[x] ^ state[x + 5] ^ state[x + 10] ^ state[x + 15] ^ state[x + 20];
        }
        for (int x = 0; x < 5; x++) {
            d[x] = c[(x + 4) % 5] ^ rotl64(c[(x + 1) % 5], 1);
        }
        for (int i = 0; i < 25; i++) {
            state[i] ^= d[i % 5];
        }

        /* Rho and Pi — lane traversal (x,y) -> (y, 2x+3y mod 5) starting at (1,0).
         * The rotation offset for step t is the triangular number ((t+1)*(t+2)/2) mod 64. */
        int x = 1, y = 0;
        uint64_t current = state[x + 5 * y];
        for (int t = 0; t < 24; t++) {
            int X = y;
            int Y = (2 * x + 3 * y) % 5;
            int idx = X + 5 * Y;
            uint64_t tmp = state[idx];
            state[idx] = rotl64(current, ((t + 1) * (t + 2) / 2) % 64);
            current = tmp;
            x = X;
            y = Y;
        }

        /* Chi */
        for (int y = 0; y < 5; y++) {
            uint64_t row[5];
            for (int x = 0; x < 5; x++) {
                row[x] = state[y * 5 + x];
            }
            for (int x = 0; x < 5; x++) {
                state[y * 5 + x] = row[x] ^ ((~row[(x + 1) % 5]) & row[(x + 2) % 5]);
            }
        }

        /* Iota */
        state[0] ^= KECCAK_RC[round];
    }
}

/* ---------------------------------------------------------------------------
 * Generic Keccak sponge (rate = 136 bytes).
 * `suffix` is the domain-separation byte placed after the data before the
 * pad10*1 padding:
 *   SHA3-256  -> 0x06
 *   SHAKE256  -> 0x1F
 *   cSHAKE256 -> 0x04
 * Absorbs `data` (len bytes), then squeezes `out_len` bytes into `output`.
 * ------------------------------------------------------------------------- */
static void keccak_sponge(const uint8_t* data, size_t len, uint8_t suffix,
                          uint8_t* output, size_t out_len) {
    uint64_t state[25];
    memset(state, 0, sizeof(state));

    /* Absorb full blocks */
    while (len >= KECCAK_RATE) {
        for (size_t i = 0; i < KECCAK_RATE; i++) {
            state[i / 8] ^= (uint64_t)data[i] << ((i % 8) * 8);
        }
        keccak_f1600(state);
        data += KECCAK_RATE;
        len -= KECCAK_RATE;
    }

    /* Final block: copy remainder, append suffix, pad10*1 */
    uint8_t block[KECCAK_RATE];
    memset(block, 0, KECCAK_RATE);
    memcpy(block, data, len);
    block[len] = suffix;
    block[KECCAK_RATE - 1] |= 0x80;
    for (size_t i = 0; i < KECCAK_RATE; i++) {
        state[i / 8] ^= (uint64_t)block[i] << ((i % 8) * 8);
    }
    keccak_f1600(state);

    /* Squeeze */
    size_t out = 0;
    while (out < out_len) {
        size_t take = (out_len - out < KECCAK_RATE) ? (out_len - out) : KECCAK_RATE;
        for (size_t i = 0; i < take; i++) {
            output[out + i] = (uint8_t)(state[i / 8] >> ((i % 8) * 8));
        }
        out += take;
        if (out < out_len) keccak_f1600(state);
    }
}

/* SHA3-256 (suffix 0x06) */
static void sha3_256(const uint8_t* input, size_t len, uint8_t* output) {
    keccak_sponge(input, len, 0x06, output, 32);
}

/* ---------------------------------------------------------------------------
 * cSHAKE256(N, data) — matches the Rust `sha3` crate's CShake256Core::new(name)
 * which sets function_name = empty and customization = name.
 *
 * Per NIST SP 800-185, when N (function name) is empty and S (customization)
 * is non-empty:
 *   cSHAKE256(X, L) = KECCAK[512](
 *       bytepad( left_encode(136) || encode_string("") || encode_string(S), 136 )
 *       || X || 0x04, L )
 *
 * encode_string(s) = left_encode(len(s) * 8) || s   (length in BITS)
 * left_encode(n) for n <= 255: [0x01, n]   (1-byte big-endian value)
 * bytepad: prepend left_encode(136) = [0x01, 0x88], then the encoded strings,
 *          then zero-pad to a multiple of 136 bytes.
 *
 * For "ProofOfWorkHash" (15 bytes = 120 bits) the 136-byte prefix is:
 *   [0x01, 0x88,  0x01, 0x00,  0x01, 0x78,  "ProofOfWorkHash"(15), 0x00...]
 *    left_enc(136) left_enc(0) left_enc(120)  customization          pad
 * ------------------------------------------------------------------------- */
static void build_cshake_prefix(const uint8_t* name, size_t name_len,
                                uint8_t prefix[KECCAK_RATE]) {
    memset(prefix, 0, KECCAK_RATE);
    size_t pos = 0;
    /* left_encode(136) = [0x01, 0x88]  (bytepad rate prefix) */
    prefix[pos++] = 0x01;
    prefix[pos++] = 0x88;
    /* encode_string("") = left_encode(0) = [0x01, 0x00]  (empty function name N) */
    prefix[pos++] = 0x01;
    prefix[pos++] = 0x00;
    /* encode_string(name) = left_encode(name_len * 8) || name  (length in bits) */
    prefix[pos++] = 0x01;
    prefix[pos++] = (uint8_t)(name_len * 8);
    /* the name (customization) bytes */
    memcpy(prefix + pos, name, name_len);
    pos += name_len;
    /* remainder is zero padding (already memset) */
}

static void cshake256(const uint8_t* name, size_t name_len,
                      const uint8_t* data, size_t data_len,
                      uint8_t* output, size_t out_len) {
    uint8_t prefix[KECCAK_RATE];
    build_cshake_prefix(name, name_len, prefix);

    /* Concatenate prefix || data and absorb with the cSHAKE suffix 0x04.
     * The combined buffer is at most 136 + (32 + 8 + 32 + 8) = 216 bytes. */
    size_t total = KECCAK_RATE + data_len;
    uint8_t* buf = (uint8_t*)malloc(total);
    if (!buf) {
        /* Allocation failure: zero output and return */
        memset(output, 0, out_len);
        return;
    }
    memcpy(buf, prefix, KECCAK_RATE);
    memcpy(buf + KECCAK_RATE, data, data_len);
    keccak_sponge(buf, total, 0x04, output, out_len);
    free(buf);
}

/* ---------------------------------------------------------------------------
 * XoShiRo256++ PRNG — matches rusty-kaspa's implementation.
 * ------------------------------------------------------------------------- */
typedef struct {
    uint64_t s[4];
} xoshiro256pp;

static void xoshiro_init(xoshiro256pp* r, const uint8_t seed[32]) {
    for (int i = 0; i < 4; i++) {
        uint64_t v = 0;
        for (int j = 0; j < 8; j++) {
            v |= (uint64_t)seed[i * 8 + j] << (j * 8);
        }
        r->s[i] = v;
    }
}

static uint64_t xoshiro_next(xoshiro256pp* r) {
    uint64_t res = r->s[0] + rotl64(r->s[0] + r->s[3], 23);
    uint64_t t = r->s[1] << 17;
    r->s[2] ^= r->s[0];
    r->s[3] ^= r->s[1];
    r->s[1] ^= r->s[2];
    r->s[0] ^= r->s[3];
    r->s[2] ^= t;
    r->s[3] = rotl64(r->s[3], 45);
    return res;
}

/* ---------------------------------------------------------------------------
 * The fixed 64x64 matrix of 4-bit values used by kHeavyHash.
 * Generated from SHA3-256("KHeavyHash") via XoShiRo256++, retrying until the
 * matrix has full rank (64) over the reals (Gaussian elimination).
 * ------------------------------------------------------------------------- */
static uint16_t g_matrix[KHEAVY_MATRIX_SIZE][KHEAVY_MATRIX_SIZE];
static int g_matrix_initialized = 0;

static void rand_matrix(xoshiro256pp* rng, uint16_t mat[KHEAVY_MATRIX_SIZE][KHEAVY_MATRIX_SIZE]) {
    for (int i = 0; i < KHEAVY_MATRIX_SIZE; i++) {
        uint64_t val = 0;
        for (int j = 0; j < KHEAVY_MATRIX_SIZE; j++) {
            int shift = j % 16;
            if (shift == 0) {
                val = xoshiro_next(rng);
            }
            mat[i][j] = (uint16_t)((val >> (4 * shift)) & 0x0F);
        }
    }
}

/* Compute the rank of the 64x64 matrix over the reals (Gaussian elimination),
 * matching rusty-kaspa's `compute_rank`. */
static int compute_rank(uint16_t mat[KHEAVY_MATRIX_SIZE][KHEAVY_MATRIX_SIZE]) {
    const double EPS = 1e-9;
    double m[KHEAVY_MATRIX_SIZE][KHEAVY_MATRIX_SIZE];
    for (int i = 0; i < KHEAVY_MATRIX_SIZE; i++) {
        for (int j = 0; j < KHEAVY_MATRIX_SIZE; j++) {
            m[i][j] = (double)mat[i][j];
        }
    }
    int rank = 0;
    int row_selected[KHEAVY_MATRIX_SIZE];
    memset(row_selected, 0, sizeof(row_selected));
    for (int i = 0; i < KHEAVY_MATRIX_SIZE; i++) {
        int j = 0;
        while (j < KHEAVY_MATRIX_SIZE) {
            double mv = m[j][i];
            double absv = mv < 0 ? -mv : mv;
            if (!row_selected[j] && absv > EPS) {
                break;
            }
            j++;
        }
        if (j != KHEAVY_MATRIX_SIZE) {
            rank++;
            row_selected[j] = 1;
            for (int p = i + 1; p < KHEAVY_MATRIX_SIZE; p++) {
                m[j][p] /= m[j][i];
            }
            for (int k = 0; k < KHEAVY_MATRIX_SIZE; k++) {
                if (k != j) {
                    double mk = m[k][i];
                    double absmk = mk < 0 ? -mk : mk;
                    if (absmk > EPS) {
                        for (int p = i + 1; p < KHEAVY_MATRIX_SIZE; p++) {
                            m[k][p] -= m[j][p] * m[k][i];
                        }
                    }
                }
            }
        }
    }
    return rank;
}

static void init_matrix(void) {
    if (g_matrix_initialized) return;

    uint8_t seed[KHEAVY_HASH_SIZE];
    sha3_256((const uint8_t*)"KHeavyHash", 10, seed);

    xoshiro256pp rng;
    xoshiro_init(&rng, seed);

    while (1) {
        uint16_t mat[KHEAVY_MATRIX_SIZE][KHEAVY_MATRIX_SIZE];
        rand_matrix(&rng, mat);
        if (compute_rank(mat) == KHEAVY_MATRIX_SIZE) {
            memcpy(g_matrix, mat, sizeof(mat));
            break;
        }
    }

    g_matrix_initialized = 1;
}

/* ---------------------------------------------------------------------------
 * Matrix-vector multiply (the "heavy" step).
 * Expands a 32-byte hash to 64 nibbles (high nibble first), multiplies by the
 * 64x64 matrix, reduces each sum to 4 bits (bits 10..13), recombines to 32
 * bytes, and XORs with the original hash.  Matches rusty-kaspa's `heavy_hash`.
 * ------------------------------------------------------------------------- */
static void heavy_hash(const uint8_t hash[KHEAVY_HASH_SIZE],
                       uint8_t product[KHEAVY_HASH_SIZE]) {
    uint8_t vec[KHEAVY_MATRIX_SIZE];
    for (int i = 0; i < KHEAVY_HASH_SIZE; i++) {
        vec[2 * i]     = (uint8_t)(hash[i] >> 4);
        vec[2 * i + 1] = (uint8_t)(hash[i] & 0x0F);
    }

    for (int i = 0; i < KHEAVY_HASH_SIZE; i++) {
        uint32_t sum1 = 0;
        uint32_t sum2 = 0;
        for (int j = 0; j < KHEAVY_MATRIX_SIZE; j++) {
            sum1 += (uint32_t)g_matrix[2 * i][j]     * (uint32_t)vec[j];
            sum2 += (uint32_t)g_matrix[2 * i + 1][j] * (uint32_t)vec[j];
        }
        product[i] = (uint8_t)(((sum1 >> 10) << 4) | (sum2 >> 10));
    }

    /* XOR with the original hash */
    for (int i = 0; i < KHEAVY_HASH_SIZE; i++) {
        product[i] ^= hash[i];
    }
}

/* ---------------------------------------------------------------------------
 * Public API
 * ------------------------------------------------------------------------- */

/* Full kHeavyHash of an arbitrary input buffer.
 * Treats `input` as the data absorbed by the ProofOfWorkHash cSHAKE (no
 * timestamp / nonce), then runs the matrix step and the HeavyHash cSHAKE. */
EXPORT void kheavyhash_hash(
    const uint8_t* input,
    size_t len,
    uint8_t* output
) {
    init_matrix();

    /* Step 1: PowHash = cSHAKE256("ProofOfWorkHash")(input) */
    uint8_t pow_hash[KHEAVY_HASH_SIZE];
    cshake256((const uint8_t*)"ProofOfWorkHash", 15, input, len, pow_hash, KHEAVY_HASH_SIZE);

    /* Step 2: matrix heavy-hash step */
    uint8_t product[KHEAVY_HASH_SIZE];
    heavy_hash(pow_hash, product);

    /* Step 3: HeavyHash = cSHAKE256("HeavyHash")(product) */
    cshake256((const uint8_t*)"HeavyHash", 9, product, KHEAVY_HASH_SIZE,
              output, KHEAVY_HASH_SIZE);
}

/* Mining hash with timestamp and nonce.
 *
 * `pre_pow_hash` is the 32-byte pre-pow hash from the pool's mining.notify,
 * `timestamp` is the block timestamp (Unix seconds), and `nonce` is the
 * 64-bit nonce.  Produces the 32-byte kHeavyHash digest. */
EXPORT void kheavyhash_mine(
    const uint8_t* pre_pow_hash,
    size_t pre_pow_hash_len,
    uint64_t timestamp,
    uint64_t nonce,
    uint8_t* output
) {
    init_matrix();

    /* Build the PowHash input:
     *   pre_pow_hash || timestamp_le || 32 zero bytes || nonce_le */
    uint8_t data[256];
    size_t off = 0;
    if (pre_pow_hash_len > 128) pre_pow_hash_len = 128; /* guard */
    memcpy(data + off, pre_pow_hash, pre_pow_hash_len);
    off += pre_pow_hash_len;
    for (int i = 0; i < 8; i++) {
        data[off++] = (uint8_t)(timestamp >> (i * 8));
    }
    for (int i = 0; i < 32; i++) {
        data[off++] = 0x00;
    }
    for (int i = 0; i < 8; i++) {
        data[off++] = (uint8_t)(nonce >> (i * 8));
    }

    /* Step 1: PowHash = cSHAKE256("ProofOfWorkHash")(data) */
    uint8_t pow_hash[KHEAVY_HASH_SIZE];
    cshake256((const uint8_t*)"ProofOfWorkHash", 15, data, off, pow_hash, KHEAVY_HASH_SIZE);

    /* Step 2: matrix heavy-hash step */
    uint8_t product[KHEAVY_HASH_SIZE];
    heavy_hash(pow_hash, product);

    /* Step 3: HeavyHash = cSHAKE256("HeavyHash")(product) */
    cshake256((const uint8_t*)"HeavyHash", 9, product, KHEAVY_HASH_SIZE,
              output, KHEAVY_HASH_SIZE);
}

/* Verify a solution against a 32-byte target (big-endian comparison). */
EXPORT int kheavyhash_verify(
    const uint8_t* pre_pow_hash,
    size_t pre_pow_hash_len,
    uint64_t timestamp,
    uint64_t nonce,
    const uint8_t* target
) {
    uint8_t hash[KHEAVY_HASH_SIZE];
    kheavyhash_mine(pre_pow_hash, pre_pow_hash_len, timestamp, nonce, hash);

    /* Compare hash <= target (big-endian, index 0 is most significant) */
    for (int i = 0; i < KHEAVY_HASH_SIZE; i++) {
        if (hash[i] < target[i]) return 1;
        if (hash[i] > target[i]) return 0;
    }
    return 1; /* equal -> meets target */
}

/* Benchmark */
EXPORT double kheavyhash_benchmark(int iterations) {
    uint8_t pre_pow_hash[32];
    memset(pre_pow_hash, 0x2A, 32);
    uint8_t output[32];

    clock_t start = clock();

    for (int i = 0; i < iterations; i++) {
        kheavyhash_mine(pre_pow_hash, 32, 5435345234ULL, (uint64_t)i, output);
    }

    clock_t end = clock();
    double seconds = (double)(end - start) / CLOCKS_PER_SEC;
    if (seconds <= 0.0) seconds = 1e-9;
    return (double)iterations / seconds;
}

/* Test */
EXPORT void kheavyhash_test(void) {
    printf("=== ZION kHeavyHash Native Library Test ===\n\n");

    uint8_t pre_pow_hash[32];
    memset(pre_pow_hash, 0x2A, 32);
    uint8_t hash[32];

    kheavyhash_mine(pre_pow_hash, 32, 5435345234ULL, 432432432ULL, hash);

    printf("Hash: ");
    for (int i = 0; i < 32; i++) printf("%02x", hash[i]);
    printf("\n\n");

    printf("Benchmark (10000 iterations)...\n");
    double hashrate = kheavyhash_benchmark(10000);
    printf("Hashrate: %.2f H/s\n", hashrate);
}

EXPORT const char* kheavyhash_version(void) {
    return "ZION kHeavyHash v2.0.0 - KAS Compatible (full matrix)";
}
