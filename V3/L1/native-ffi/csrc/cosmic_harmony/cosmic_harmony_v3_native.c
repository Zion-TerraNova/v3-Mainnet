/*
 * ZION Cosmic Harmony v3 - Native C Implementation
 * 
 * Full CHv3 pipeline: Keccak-256 → SHA3-512 → Golden Matrix → Cosmic Fusion
 * Pure C, no external dependencies. ARM NEON optimized for Apple Silicon.
 *
 * Algorithm (must match Rust implementation in algorithms_opt.rs):
 *   1. Input = header[0:80] + nonce(8 bytes LE) = 88 bytes
 *   2. Step 1: Keccak-256(input) → 32 bytes
 *   3. Step 2: SHA3-512(step1) → 64 bytes
 *   4. Step 3: Golden Matrix(step2) → 64 bytes (8×8 matrix × PHI_POWERS_FP)
 *   5. Step 4: Cosmic Fusion(step3) → 32 bytes (4 rounds Keccak+XOR + final SHA3-512)
 *
 * Build:
 *   macOS ARM:  clang -O3 -shared -fPIC cosmic_harmony_v3_native.c -o libcosmic_harmony_v3.dylib
 *   macOS x86:  clang -O3 -shared -fPIC -mavx2 cosmic_harmony_v3_native.c -o libcosmic_harmony_v3.dylib
 *   Linux:      gcc -O3 -shared -fPIC cosmic_harmony_v3_native.c -o libcosmic_harmony_v3.so
 *   Windows:    cl /O2 cosmic_harmony_v3_native.c /LD /Fe:cosmic_harmony_v3.dll
 *
 * Author: ZION AI Native Team
 * Version: 2.9.5
 * Date: February 2026
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <time.h>

/* ============================================================================
 * Platform Detection & Macros
 * ============================================================================ */

#ifdef _MSC_VER
    #define EXPORT __declspec(dllexport)
    #define ALIGNED(x) __declspec(align(x))
#elif defined(__MINGW32__) || defined(__MINGW64__)
    #define EXPORT __attribute__((visibility("default")))
    #define ALIGNED(x) __attribute__((aligned(x)))
#else
    #define EXPORT __attribute__((visibility("default")))
    #define ALIGNED(x) __attribute__((aligned(x)))
#endif

#if defined(__aarch64__) || defined(__arm__) || defined(_M_ARM) || defined(_M_ARM64)
    #ifdef __ARM_NEON
        #include <arm_neon.h>
        #define HAS_NEON 1
    #else
        #define HAS_NEON 0
    #endif
    #define HAS_AVX2 0
#elif defined(__AVX2__)
    #include <immintrin.h>
    #define HAS_AVX2 1
    #define HAS_NEON 0
#else
    #define HAS_AVX2 0
    #define HAS_NEON 0
#endif

/* ============================================================================
 * Keccak / SHA3 Constants
 * ============================================================================ */

/* Keccak round constants (24 rounds) */
static const uint64_t KECCAK_RC[24] = {
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
    0x0000000080000001ULL, 0x8000000080008008ULL
};

/* Keccak rotation offsets */
static const int KECCAK_ROTC[24] = {
    1,  3,  6,  10, 15, 21, 28, 36,
    45, 55, 2,  14, 27, 41, 56, 8,
    25, 43, 62, 18, 39, 61, 20, 44
};

/* Keccak pi lane indices */
static const int KECCAK_PILN[24] = {
    10, 7,  11, 17, 18, 3,  5,  16,
    8,  21, 24, 4,  15, 23, 19, 13,
    12, 2,  20, 14, 22, 9,  6,  1
};

/* ============================================================================
 * CHv3 Constants (must match Rust exactly)
 * ============================================================================ */

/* Fixed-point golden ratio powers: PHI^n * 2^32 */
static const uint64_t PHI_POWERS_FP[16] = {
    4294967296ULL,      /* φ^0  * 2^32 */
    6949403065ULL,      /* φ^1  * 2^32 */
    11244370361ULL,     /* φ^2  * 2^32 */
    18193773427ULL,     /* φ^3  * 2^32 */
    29438143788ULL,     /* φ^4  * 2^32 */
    47631917215ULL,     /* φ^5  * 2^32 */
    77070061004ULL,     /* φ^6  * 2^32 */
    124701978219ULL,    /* φ^7  * 2^32 */
    201772039223ULL,    /* φ^8  * 2^32 */
    326474017443ULL,    /* φ^9  * 2^32 */
    528246056666ULL,    /* φ^10 * 2^32 */
    854720074109ULL,    /* φ^11 * 2^32 */
    1382966130776ULL,   /* φ^12 * 2^32 */
    2237686204885ULL,   /* φ^13 * 2^32 */
    3620652335660ULL,   /* φ^14 * 2^32 */
    5858338540545ULL    /* φ^15 * 2^32 */
};

/* Cosmic XOR mask (repeating pattern 0x74, 0x9D, 0x30, 0x60) */
static const uint8_t COSMIC_XOR_MASK[32] = {
    0x74, 0x9D, 0x30, 0x60, 0x74, 0x9D, 0x30, 0x60,
    0x74, 0x9D, 0x30, 0x60, 0x74, 0x9D, 0x30, 0x60,
    0x74, 0x9D, 0x30, 0x60, 0x74, 0x9D, 0x30, 0x60,
    0x74, 0x9D, 0x30, 0x60, 0x74, 0x9D, 0x30, 0x60
};

/* ============================================================================
 * Keccak-f[1600] Permutation (24 rounds)
 * ============================================================================ */

static inline uint64_t rotl64(uint64_t x, int n) {
    return (x << n) | (x >> (64 - n));
}

static void keccak_f1600(uint64_t state[25]) {
    uint64_t bc[5];
    uint64_t t;
    
    for (int round = 0; round < 24; round++) {
        /* θ step */
        for (int i = 0; i < 5; i++) {
            bc[i] = state[i] ^ state[i + 5] ^ state[i + 10] ^ state[i + 15] ^ state[i + 20];
        }
        for (int i = 0; i < 5; i++) {
            t = bc[(i + 4) % 5] ^ rotl64(bc[(i + 1) % 5], 1);
            for (int j = 0; j < 25; j += 5) {
                state[j + i] ^= t;
            }
        }
        
        /* ρ and π steps */
        t = state[1];
        for (int i = 0; i < 24; i++) {
            int j = KECCAK_PILN[i];
            bc[0] = state[j];
            state[j] = rotl64(t, KECCAK_ROTC[i]);
            t = bc[0];
        }
        
        /* χ step */
        for (int j = 0; j < 25; j += 5) {
            for (int i = 0; i < 5; i++) {
                bc[i] = state[j + i];
            }
            for (int i = 0; i < 5; i++) {
                state[j + i] ^= (~bc[(i + 1) % 5]) & bc[(i + 2) % 5];
            }
        }
        
        /* ι step */
        state[0] ^= KECCAK_RC[round];
    }
}

/* ============================================================================
 * Keccak-256 (FIPS 202 - Keccak, NOT SHA3 padding)
 * Rate = 136 bytes (1088 bits), capacity = 64 bytes (512 bits)
 * Keccak uses padding: 0x01 ... 0x80
 * ============================================================================ */

static void keccak256(const uint8_t *input, size_t input_len, uint8_t output[32]) {
    uint64_t state[25];
    memset(state, 0, sizeof(state));
    
    const size_t rate = 136; /* bytes */
    size_t offset = 0;
    
    /* Absorb full blocks */
    while (offset + rate <= input_len) {
        for (size_t i = 0; i < rate / 8; i++) {
            uint64_t word = 0;
            for (int j = 0; j < 8; j++) {
                word |= ((uint64_t)input[offset + i * 8 + j]) << (j * 8);
            }
            state[i] ^= word;
        }
        keccak_f1600(state);
        offset += rate;
    }
    
    /* Absorb final block with padding */
    uint8_t block[200]; /* max rate size */
    memset(block, 0, rate);
    size_t remaining = input_len - offset;
    if (remaining > 0) {
        memcpy(block, input + offset, remaining);
    }
    
    /* Keccak padding (0x01 ... 0x80) — NOT SHA3 (0x06 ... 0x80) */
    block[remaining] = 0x01;
    block[rate - 1] |= 0x80;
    
    for (size_t i = 0; i < rate / 8; i++) {
        uint64_t word = 0;
        for (int j = 0; j < 8; j++) {
            word |= ((uint64_t)block[i * 8 + j]) << (j * 8);
        }
        state[i] ^= word;
    }
    keccak_f1600(state);
    
    /* Squeeze output (32 bytes) */
    for (int i = 0; i < 4; i++) {
        for (int j = 0; j < 8; j++) {
            output[i * 8 + j] = (uint8_t)(state[i] >> (j * 8));
        }
    }
}

/* ============================================================================
 * SHA3-512 (FIPS 202)
 * Rate = 72 bytes (576 bits), capacity = 128 bytes (1024 bits)
 * SHA3 uses padding: 0x06 ... 0x80
 * ============================================================================ */

static void sha3_512(const uint8_t *input, size_t input_len, uint8_t output[64]) {
    uint64_t state[25];
    memset(state, 0, sizeof(state));
    
    const size_t rate = 72; /* bytes */
    size_t offset = 0;
    
    /* Absorb full blocks */
    while (offset + rate <= input_len) {
        for (size_t i = 0; i < rate / 8; i++) {
            uint64_t word = 0;
            for (int j = 0; j < 8; j++) {
                word |= ((uint64_t)input[offset + i * 8 + j]) << (j * 8);
            }
            state[i] ^= word;
        }
        keccak_f1600(state);
        offset += rate;
    }
    
    /* Absorb final block with SHA3 padding (0x06 ... 0x80) */
    uint8_t block[200];
    memset(block, 0, rate);
    size_t remaining = input_len - offset;
    if (remaining > 0) {
        memcpy(block, input + offset, remaining);
    }
    
    /* SHA3 domain separation: 0x06 */
    block[remaining] = 0x06;
    block[rate - 1] |= 0x80;
    
    for (size_t i = 0; i < rate / 8; i++) {
        uint64_t word = 0;
        for (int j = 0; j < 8; j++) {
            word |= ((uint64_t)block[i * 8 + j]) << (j * 8);
        }
        state[i] ^= word;
    }
    keccak_f1600(state);
    
    /* Squeeze output (64 bytes) */
    for (int i = 0; i < 8; i++) {
        for (int j = 0; j < 8; j++) {
            output[i * 8 + j] = (uint8_t)(state[i] >> (j * 8));
        }
    }
}

/* ============================================================================
 * Golden Matrix (Step 3) — Fixed-point integer arithmetic
 * Must match Rust golden_matrix_opt() exactly!
 * ============================================================================ */

static void golden_matrix(const uint8_t input[64], uint8_t output[64]) {
    const int MATRIX_SIZE = 8;
    uint64_t matrix[8][8];
    uint64_t result[8];
    
    /* Fill 8×8 matrix from input bytes */
    for (int i = 0; i < MATRIX_SIZE; i++) {
        int base = i * MATRIX_SIZE;
        for (int j = 0; j < MATRIX_SIZE; j++) {
            matrix[i][j] = (uint64_t)input[(base + j) % 64];
        }
    }
    
    /* Apply golden ratio with fixed-point integer powers */
    for (int i = 0; i < MATRIX_SIZE; i++) {
        /* Use 128-bit arithmetic for precision */
        /* sum = Σ(matrix[i][j] * PHI_POWERS_FP[i+j]) */
        /* Then shift right by 32 to get integer part */
        
#ifdef __SIZEOF_INT128__
        /* Native 128-bit support (GCC/Clang) */
        __uint128_t sum = 0;
        sum += (__uint128_t)matrix[i][0] * (__uint128_t)PHI_POWERS_FP[i + 0];
        sum += (__uint128_t)matrix[i][1] * (__uint128_t)PHI_POWERS_FP[i + 1];
        sum += (__uint128_t)matrix[i][2] * (__uint128_t)PHI_POWERS_FP[i + 2];
        sum += (__uint128_t)matrix[i][3] * (__uint128_t)PHI_POWERS_FP[i + 3];
        sum += (__uint128_t)matrix[i][4] * (__uint128_t)PHI_POWERS_FP[i + 4];
        sum += (__uint128_t)matrix[i][5] * (__uint128_t)PHI_POWERS_FP[i + 5];
        sum += (__uint128_t)matrix[i][6] * (__uint128_t)PHI_POWERS_FP[i + 6];
        sum += (__uint128_t)matrix[i][7] * (__uint128_t)PHI_POWERS_FP[i + 7];
        
        /* Shift right by 32 to get integer part (matches Rust >> 32) */
        result[i] = (uint64_t)(sum >> 32);
#else
        /* Fallback: manual 128-bit using 64-bit ops */
        uint64_t sum_lo = 0, sum_hi = 0;
        for (int j = 0; j < MATRIX_SIZE; j++) {
            uint64_t a = matrix[i][j];
            uint64_t b = PHI_POWERS_FP[i + j];
            /* Multiply: a * b = (a_lo * b_lo) + ... */
            uint64_t a_lo = a & 0xFFFFFFFF;
            uint64_t a_hi = a >> 32;
            uint64_t b_lo = b & 0xFFFFFFFF;
            uint64_t b_hi = b >> 32;
            
            uint64_t p0 = a_lo * b_lo;
            uint64_t p1 = a_lo * b_hi;
            uint64_t p2 = a_hi * b_lo;
            uint64_t p3 = a_hi * b_hi;
            
            uint64_t mid = p1 + (p0 >> 32);
            mid += p2;
            if (mid < p2) sum_hi++;  /* carry */
            
            uint64_t lo = (p0 & 0xFFFFFFFF) | ((mid & 0xFFFFFFFF) << 32);
            uint64_t hi = p3 + (mid >> 32);
            
            uint64_t old_lo = sum_lo;
            sum_lo += lo;
            if (sum_lo < old_lo) sum_hi++;  /* carry */
            sum_hi += hi;
        }
        /* Shift right by 32 */
        result[i] = (sum_lo >> 32) | (sum_hi << 32);
#endif
    }
    
    /* Convert result to bytes (LE) — matches Rust val.to_le_bytes() */
    for (int i = 0; i < 8; i++) {
        uint64_t val = result[i];
        output[i * 8 + 0] = (uint8_t)(val >>  0);
        output[i * 8 + 1] = (uint8_t)(val >>  8);
        output[i * 8 + 2] = (uint8_t)(val >> 16);
        output[i * 8 + 3] = (uint8_t)(val >> 24);
        output[i * 8 + 4] = (uint8_t)(val >> 32);
        output[i * 8 + 5] = (uint8_t)(val >> 40);
        output[i * 8 + 6] = (uint8_t)(val >> 48);
        output[i * 8 + 7] = (uint8_t)(val >> 56);
    }
}

/* ============================================================================
 * Cosmic Fusion (Step 4) — 4 rounds of Keccak+XOR, final SHA3-512
 * Must match Rust cosmic_fusion_opt() exactly!
 * ============================================================================ */

static void fusion_round(uint8_t state[64], uint8_t round_num) {
    /* Keccak-256 of state[0:32] + round_byte */
    uint8_t keccak_input[33];
    memcpy(keccak_input, state, 32);
    keccak_input[32] = round_num;
    
    uint8_t intermediate[32];
    keccak256(keccak_input, 33, intermediate);
    
    /* XOR with COSMIC_XOR_MASK into state[0:32] */
    for (int i = 0; i < 32; i++) {
        state[i] = intermediate[i] ^ COSMIC_XOR_MASK[i];
    }
}

static void cosmic_fusion(const uint8_t input[64], uint8_t output[32]) {
    /* Copy input to state buffer */
    uint8_t state[64];
    memcpy(state, input, 64);
    
    /* 4 rounds of fusion */
    fusion_round(state, 0);
    fusion_round(state, 1);
    fusion_round(state, 2);
    fusion_round(state, 3);
    
    /* Final SHA3-512 of state[0:32], truncate to 32 bytes */
    uint8_t full[64];
    sha3_512(state, 32, full);
    memcpy(output, full, 32);
}

/* ============================================================================
 * Full CHv3 Pipeline
 * ============================================================================ */

static void cosmic_harmony_v3_compute(
    const uint8_t *block_header,
    size_t header_len,
    uint64_t nonce,
    uint8_t output[32]
) {
    /* Prepare input: header[0:80] + nonce(8 bytes LE) = 88 bytes */
    uint8_t input[88];
    memset(input, 0, 88);
    size_t copy_len = header_len < 80 ? header_len : 80;
    memcpy(input, block_header, copy_len);
    
    /* Append nonce as little-endian 8 bytes */
    input[80] = (uint8_t)(nonce >>  0);
    input[81] = (uint8_t)(nonce >>  8);
    input[82] = (uint8_t)(nonce >> 16);
    input[83] = (uint8_t)(nonce >> 24);
    input[84] = (uint8_t)(nonce >> 32);
    input[85] = (uint8_t)(nonce >> 40);
    input[86] = (uint8_t)(nonce >> 48);
    input[87] = (uint8_t)(nonce >> 56);
    
    /* Step 1: Keccak-256 */
    uint8_t step1[32];
    keccak256(input, 88, step1);
    
    /* Step 2: SHA3-512 */
    uint8_t step2[64];
    sha3_512(step1, 32, step2);
    
    /* Step 3: Golden Matrix */
    uint8_t step3[64];
    golden_matrix(step2, step3);
    
    /* Step 4: Cosmic Fusion */
    cosmic_fusion(step3, output);
}

/* ============================================================================
 * GPU Mining State (for Metal integration)
 * ============================================================================ */

typedef struct {
    uint8_t header[80];
    size_t header_len;
    uint32_t batch_size;
    uint32_t device_id;
    int initialized;
} CHv3GPUState;

static CHv3GPUState g_gpu_state = {0};

/* Forward declarations */
EXPORT const char* cosmic_harmony_v3_get_info(void);

/* ============================================================================
 * Public API — Exported Functions
 * ============================================================================ */

/* Get number of available GPU devices */
EXPORT uint32_t cosmic_harmony_v3_gpu_count(void) {
    /* On macOS with Metal, we have at least 1 GPU (Apple Silicon) */
#if defined(__APPLE__)
    return 1;
#else
    return 0;
#endif
}

/* Initialize GPU mining context */
EXPORT int32_t cosmic_harmony_v3_gpu_init(uint32_t device_id, uint32_t batch_size) {
    g_gpu_state.device_id = device_id;
    g_gpu_state.batch_size = batch_size;
    g_gpu_state.initialized = 1;
    
    printf("[CHv3 Native] GPU init: device=%u, batch_size=%u\n", device_id, batch_size);
    printf("[CHv3 Native] Library: %s\n", cosmic_harmony_v3_get_info());
    
    return 0;  /* Success */
}

/* Mine a batch of nonces — CPU fallback (Metal version in .metal shader) */
EXPORT int32_t cosmic_harmony_v3_gpu_mine(
    const uint8_t *header,
    size_t header_len,
    uint64_t nonce_start,
    const uint8_t *target,
    uint64_t *found_nonce,
    uint8_t *found_hash
) {
    if (!g_gpu_state.initialized) return -1;
    
    uint32_t batch = g_gpu_state.batch_size;
    
    for (uint32_t i = 0; i < batch; i++) {
        uint64_t nonce = nonce_start + i;
        uint8_t hash[32];
        
        cosmic_harmony_v3_compute(header, header_len, nonce, hash);
        
        /* Check against target (compare as big-endian 256-bit number) */
        /* Hash bytes are compared from index 31 (MSB) down to 0 (LSB) */
        int below_target = 0;
        for (int j = 31; j >= 0; j--) {
            if (hash[j] < target[j]) {
                below_target = 1;
                break;
            } else if (hash[j] > target[j]) {
                break;
            }
        }
        
        if (below_target) {
            *found_nonce = nonce;
            memcpy(found_hash, hash, 32);
            return 1;  /* Found */
        }
    }
    
    return 0;  /* Not found in this batch */
}

/* Cleanup GPU resources */
EXPORT void cosmic_harmony_v3_gpu_cleanup(void) {
    g_gpu_state.initialized = 0;
    printf("[CHv3 Native] GPU cleanup done\n");
}

/* ============================================================================
 * Simple hash function (for verification / pool share validation)
 * ============================================================================ */

EXPORT int cosmic_harmony_v3_hash(
    const uint8_t *header,
    size_t header_len,
    uint64_t nonce,
    uint8_t *output
) {
    if (!header || !output) return -1;
    
    cosmic_harmony_v3_compute(header, header_len, nonce, output);
    return 0;
}

/* Hash without nonce (raw input) */
EXPORT int cosmic_harmony_v3_hash_raw(
    const uint8_t *input,
    size_t input_len,
    uint8_t *output
) {
    if (!input || !output) return -1;
    
    /* Step 1: Keccak-256 */
    uint8_t step1[32];
    keccak256(input, input_len, step1);
    
    /* Step 2: SHA3-512 */
    uint8_t step2[64];
    sha3_512(step1, 32, step2);
    
    /* Step 3: Golden Matrix */
    uint8_t step3[64];
    golden_matrix(step2, step3);
    
    /* Step 4: Cosmic Fusion */
    cosmic_fusion(step3, output);
    
    return 0;
}

/* ============================================================================
 * Info & Benchmark
 * ============================================================================ */

EXPORT const char* cosmic_harmony_v3_get_info(void) {
#if HAS_NEON
    return "Cosmic Harmony v3 Native (ARM NEON - Apple Silicon)";
#elif HAS_AVX2
    return "Cosmic Harmony v3 Native (x86_64 AVX2)";
#else
    return "Cosmic Harmony v3 Native (scalar)";
#endif
}

EXPORT int cosmic_harmony_v3_has_neon(void) {
#if HAS_NEON
    return 1;
#else
    return 0;
#endif
}

EXPORT int cosmic_harmony_v3_has_avx2(void) {
#if HAS_AVX2
    return 1;
#else
    return 0;
#endif
}

/* Individual step exports (for debugging / verification) */
EXPORT void cosmic_harmony_v3_keccak256(const uint8_t *input, size_t len, uint8_t *output) {
    keccak256(input, len, output);
}

EXPORT void cosmic_harmony_v3_sha3_512(const uint8_t *input, size_t len, uint8_t *output) {
    sha3_512(input, len, output);
}

EXPORT void cosmic_harmony_v3_golden_matrix(const uint8_t *input, uint8_t *output) {
    golden_matrix(input, output);
}

EXPORT void cosmic_harmony_v3_cosmic_fusion(const uint8_t *input, uint8_t *output) {
    cosmic_fusion(input, output);
}

/* Benchmark */
EXPORT double cosmic_harmony_v3_benchmark(int duration_seconds) {
    uint8_t header[80];
    uint8_t output[32];
    memset(header, 0x42, 80);
    
    printf("=== Cosmic Harmony v3 Benchmark ===\n");
    printf("Library: %s\n", cosmic_harmony_v3_get_info());
    printf("Running for %d seconds...\n", duration_seconds);
    
    uint64_t total_hashes = 0;
    uint64_t nonce = 0;
    
    clock_t start = clock();
    double elapsed = 0.0;
    
    while (elapsed < duration_seconds) {
        cosmic_harmony_v3_compute(header, 80, nonce++, output);
        total_hashes++;
        
        if ((total_hashes % 100) == 0) {
            elapsed = (double)(clock() - start) / CLOCKS_PER_SEC;
        }
    }
    
    elapsed = (double)(clock() - start) / CLOCKS_PER_SEC;
    
    double hashrate = total_hashes / elapsed;
    printf("Total hashes: %llu\n", (unsigned long long)total_hashes);
    printf("Time: %.2f s\n", elapsed);
    printf("Hashrate: %.2f H/s\n", hashrate);
    printf("Sample hash: ");
    for (int i = 0; i < 32; i++) printf("%02x", output[i]);
    printf("\n");
    
    return hashrate;
}

/* ============================================================================
 * Main (for standalone testing)
 * ============================================================================ */

#ifdef BUILD_MAIN
int main(int argc, char **argv) {
    printf("=== ZION Cosmic Harmony v3 Native Library ===\n");
    printf("%s\n\n", cosmic_harmony_v3_get_info());
    
    /* Test vector: "ZION block header v2.9.5" with nonce=12345 */
    const char *test_header = "ZION block header v2.9.5";
    uint8_t hash[32];
    
    cosmic_harmony_v3_compute((const uint8_t*)test_header, strlen(test_header), 12345, hash);
    
    printf("Test header: \"%s\"\n", test_header);
    printf("Nonce: 12345\n");
    printf("Hash: ");
    for (int i = 0; i < 32; i++) printf("%02x", hash[i]);
    printf("\n\n");
    
    /* Individual step test */
    uint8_t input88[88];
    memset(input88, 0, 88);
    memcpy(input88, test_header, strlen(test_header));
    uint64_t nonce = 12345;
    input88[80] = (uint8_t)(nonce >>  0);
    input88[81] = (uint8_t)(nonce >>  8);
    input88[82] = (uint8_t)(nonce >> 16);
    input88[83] = (uint8_t)(nonce >> 24);
    input88[84] = (uint8_t)(nonce >> 32);
    input88[85] = (uint8_t)(nonce >> 40);
    input88[86] = (uint8_t)(nonce >> 48);
    input88[87] = (uint8_t)(nonce >> 56);
    
    uint8_t step1[32];
    keccak256(input88, 88, step1);
    printf("Step 1 (Keccak-256): ");
    for (int i = 0; i < 32; i++) printf("%02x", step1[i]);
    printf("\n");
    
    uint8_t step2[64];
    sha3_512(step1, 32, step2);
    printf("Step 2 (SHA3-512):   ");
    for (int i = 0; i < 32; i++) printf("%02x", step2[i]);
    printf("...\n");
    
    uint8_t step3[64];
    golden_matrix(step2, step3);
    printf("Step 3 (Golden Mat): ");
    for (int i = 0; i < 32; i++) printf("%02x", step3[i]);
    printf("...\n");
    
    uint8_t step4[32];
    cosmic_fusion(step3, step4);
    printf("Step 4 (Cosmic Fus): ");
    for (int i = 0; i < 32; i++) printf("%02x", step4[i]);
    printf("\n\n");
    
    /* Benchmark */
    cosmic_harmony_v3_benchmark(5);
    
    return 0;
}
#endif
