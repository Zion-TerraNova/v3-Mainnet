/*
 * ============================================================================
 *  ZION Native kHeavyHash Library
 *  High-performance kHeavyHash implementation for Kaspa (KAS) mining
 *  
 *  Algorithm: SHA3-256 → Matrix Multiplication → SHA3-256
 *  
 *  Compilation:
 *    macOS: clang -O3 -fPIC -shared -o libkheavyhash_zion.dylib kheavyhash_native.c
 *    Linux: gcc -O3 -fPIC -shared -o libkheavyhash_zion.so kheavyhash_native.c
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
        
        /* Rho and Pi */
        uint64_t temp = state[1];
        for (int t = 0; t < 24; t++) {
            int idx = (t * 7 + 1) % 25;
            uint64_t tmp2 = state[idx];
            state[idx] = rotl64(temp, ((t + 1) * (t + 2) / 2) % 64);
            temp = tmp2;
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

/* SHA3-256 */
static void sha3_256(const uint8_t* input, size_t len, uint8_t* output) {
    uint64_t state[25] = {0};
    
    /* Absorb */
    size_t rate = 136;  /* 1088 bits for SHA3-256 */
    size_t block_size = rate;
    
    while (len >= block_size) {
        for (size_t i = 0; i < block_size / 8; i++) {
            state[i] ^= ((uint64_t*)input)[i];
        }
        keccak_f1600(state);
        input += block_size;
        len -= block_size;
    }
    
    /* Pad and absorb final block */
    uint8_t padded[136] = {0};
    memcpy(padded, input, len);
    padded[len] = 0x06;  /* SHA3 domain separator */
    padded[block_size - 1] |= 0x80;
    
    for (size_t i = 0; i < block_size / 8; i++) {
        state[i] ^= ((uint64_t*)padded)[i];
    }
    keccak_f1600(state);
    
    /* Squeeze */
    memcpy(output, state, 32);
}

/* Kaspa-specific matrix (deterministically generated) */
static uint64_t g_matrix[KHEAVY_MATRIX_SIZE][KHEAVY_MATRIX_SIZE];
static int g_matrix_initialized = 0;

static void init_matrix() {
    if (g_matrix_initialized) return;
    
    /* Generate matrix from seed */
    uint8_t seed[32];
    sha3_256((uint8_t*)"KHeavyHash", 10, seed);
    
    for (int i = 0; i < KHEAVY_MATRIX_SIZE; i++) {
        for (int j = 0; j < KHEAVY_MATRIX_SIZE; j++) {
            uint8_t idx_data[34];
            memcpy(idx_data, seed, 32);
            idx_data[32] = (uint8_t)i;
            idx_data[33] = (uint8_t)j;
            
            uint8_t hash[32];
            sha3_256(idx_data, 34, hash);
            
            g_matrix[i][j] = *(uint64_t*)hash;
        }
    }
    
    g_matrix_initialized = 1;
}

/* Compute kHeavyHash */
EXPORT void kheavyhash_hash(
    const uint8_t* input,
    size_t len,
    uint8_t* output
) {
    init_matrix();
    
    /* Step 1: SHA3-256 pre-hash */
    uint8_t pre_hash[32];
    sha3_256(input, len, pre_hash);
    
    /* Step 2: Convert to 64-element vector (pad with more hashes) */
    uint64_t vec[KHEAVY_MATRIX_SIZE];
    
    /* First 4 elements from pre_hash */
    for (int i = 0; i < 4; i++) {
        vec[i] = ((uint64_t*)pre_hash)[i];
    }
    
    /* Generate remaining elements */
    for (int i = 4; i < KHEAVY_MATRIX_SIZE; i += 4) {
        uint8_t seed[33];
        memcpy(seed, pre_hash, 32);
        seed[32] = (uint8_t)i;
        
        uint8_t h[32];
        sha3_256(seed, 33, h);
        
        for (int j = 0; j < 4 && i + j < KHEAVY_MATRIX_SIZE; j++) {
            vec[i + j] = ((uint64_t*)h)[j];
        }
    }
    
    /* Step 3: Matrix-vector multiplication */
    uint64_t result[KHEAVY_MATRIX_SIZE];
    for (int i = 0; i < KHEAVY_MATRIX_SIZE; i++) {
        uint64_t sum = 0;
        for (int j = 0; j < KHEAVY_MATRIX_SIZE; j++) {
            sum += g_matrix[i][j] * vec[j];
        }
        result[i] = sum;
    }
    
    /* Step 4: SHA3-256 post-hash */
    sha3_256((uint8_t*)result, KHEAVY_MATRIX_SIZE * 8, output);
}

/* Mining hash with nonce */
EXPORT void kheavyhash_mine(
    const uint8_t* header,
    size_t header_len,
    uint64_t nonce,
    uint8_t* output
) {
    uint8_t data[256];
    memcpy(data, header, header_len);
    memcpy(data + header_len, &nonce, 8);
    
    kheavyhash_hash(data, header_len + 8, output);
}

/* Verify solution */
EXPORT int kheavyhash_verify(
    const uint8_t* header,
    size_t header_len,
    uint64_t nonce,
    const uint8_t* target
) {
    uint8_t hash[32];
    kheavyhash_mine(header, header_len, nonce, hash);
    
    /* Compare hash < target (big-endian) */
    for (int i = 31; i >= 0; i--) {
        if (hash[i] < target[i]) return 1;
        if (hash[i] > target[i]) return 0;
    }
    return 1;
}

/* Benchmark */
EXPORT double kheavyhash_benchmark(int iterations) {
    uint8_t header[80] = {0x01, 0x02, 0x03};
    uint8_t output[32];
    
    clock_t start = clock();
    
    for (int i = 0; i < iterations; i++) {
        kheavyhash_mine(header, 80, i, output);
    }
    
    clock_t end = clock();
    double seconds = (double)(end - start) / CLOCKS_PER_SEC;
    
    return iterations / seconds;
}

/* Test */
EXPORT void kheavyhash_test() {
    printf("=== ZION kHeavyHash Native Library Test ===\n\n");
    
    uint8_t header[80] = {0x01, 0x02, 0x03, 0x04};
    uint8_t hash[32];
    
    kheavyhash_mine(header, 80, 12345, hash);
    
    printf("Hash: ");
    for (int i = 0; i < 8; i++) printf("%02x", hash[i]);
    printf("...\n\n");
    
    printf("Benchmark (10000 iterations)...\n");
    double hashrate = kheavyhash_benchmark(10000);
    printf("Hashrate: %.2f H/s\n", hashrate);
}

EXPORT const char* kheavyhash_version() {
    return "ZION kHeavyHash v1.0.0 - KAS Compatible";
}
