/*
 * ============================================================================
 *  ZION Native Blake3 Library
 *  Blake3 implementation for ALPH (Alephium) mining
 *  
 *  Algorithm: Blake3 - Fast cryptographic hash
 *  
 *  Compilation:
 *    macOS: clang -O3 -fPIC -shared -o libblake3_zion.dylib blake3_native.c
 *    Linux: gcc -O3 -fPIC -shared -o libblake3_zion.so blake3_native.c
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

/* Blake3 constants */
#define BLAKE3_BLOCK_LEN     64
#define BLAKE3_CHUNK_LEN     1024
#define BLAKE3_KEY_LEN       32
#define BLAKE3_OUT_LEN       32
#define BLAKE3_MAX_DEPTH     54

/* Blake3 IV */
static const uint32_t BLAKE3_IV[8] = {
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A,
    0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19
};

/* Blake3 flags */
#define CHUNK_START         (1 << 0)
#define CHUNK_END           (1 << 1)
#define PARENT              (1 << 2)
#define ROOT                (1 << 3)
#define KEYED_HASH          (1 << 4)

/* Message schedule */
static const uint8_t MSG_SCHEDULE[7][16] = {
    {0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15},
    {2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8},
    {3, 4, 10, 12, 13, 2, 7, 14, 6, 5, 9, 0, 11, 15, 8, 1},
    {10, 7, 12, 9, 14, 3, 13, 15, 4, 0, 11, 2, 5, 8, 1, 6},
    {12, 13, 9, 11, 15, 10, 14, 8, 7, 2, 5, 3, 0, 1, 6, 4},
    {9, 14, 11, 5, 8, 12, 15, 1, 13, 3, 0, 10, 2, 6, 4, 7},
    {11, 15, 5, 0, 1, 9, 8, 6, 14, 10, 2, 12, 3, 4, 7, 13}
};

/* Rotate right */
static inline uint32_t rotr32(uint32_t x, int n) {
    return (x >> n) | (x << (32 - n));
}

/* G function */
static inline void g(uint32_t* state, int a, int b, int c, int d, uint32_t mx, uint32_t my) {
    state[a] = state[a] + state[b] + mx;
    state[d] = rotr32(state[d] ^ state[a], 16);
    state[c] = state[c] + state[d];
    state[b] = rotr32(state[b] ^ state[c], 12);
    state[a] = state[a] + state[b] + my;
    state[d] = rotr32(state[d] ^ state[a], 8);
    state[c] = state[c] + state[d];
    state[b] = rotr32(state[b] ^ state[c], 7);
}

/* Round function */
static void round_fn(uint32_t state[16], const uint32_t msg[16], int round) {
    const uint8_t* schedule = MSG_SCHEDULE[round % 7];
    
    /* Column rounds */
    g(state, 0, 4, 8,  12, msg[schedule[0]],  msg[schedule[1]]);
    g(state, 1, 5, 9,  13, msg[schedule[2]],  msg[schedule[3]]);
    g(state, 2, 6, 10, 14, msg[schedule[4]],  msg[schedule[5]]);
    g(state, 3, 7, 11, 15, msg[schedule[6]],  msg[schedule[7]]);
    
    /* Diagonal rounds */
    g(state, 0, 5, 10, 15, msg[schedule[8]],  msg[schedule[9]]);
    g(state, 1, 6, 11, 12, msg[schedule[10]], msg[schedule[11]]);
    g(state, 2, 7, 8,  13, msg[schedule[12]], msg[schedule[13]]);
    g(state, 3, 4, 9,  14, msg[schedule[14]], msg[schedule[15]]);
}

/* Compress function */
static void blake3_compress(
    const uint32_t cv[8],
    const uint8_t block[BLAKE3_BLOCK_LEN],
    uint8_t block_len,
    uint64_t counter,
    uint8_t flags,
    uint32_t out[16]
) {
    uint32_t state[16];
    uint32_t msg[16];
    
    /* Initialize state */
    memcpy(state, cv, 8 * sizeof(uint32_t));
    memcpy(state + 8, BLAKE3_IV, 4 * sizeof(uint32_t));
    state[12] = (uint32_t)counter;
    state[13] = (uint32_t)(counter >> 32);
    state[14] = block_len;
    state[15] = flags;
    
    /* Parse message */
    for (int i = 0; i < 16; i++) {
        msg[i] = (block[4*i]) | (block[4*i + 1] << 8) | 
                 (block[4*i + 2] << 16) | (block[4*i + 3] << 24);
    }
    
    /* 7 rounds */
    for (int i = 0; i < 7; i++) {
        round_fn(state, msg, i);
    }
    
    /* Finalize */
    for (int i = 0; i < 8; i++) {
        state[i] ^= state[i + 8];
        state[i + 8] ^= cv[i];
    }
    
    memcpy(out, state, 16 * sizeof(uint32_t));
}

/* Blake3 context */
typedef struct {
    uint32_t key[8];
    uint32_t cv[8];
    uint8_t buf[BLAKE3_BLOCK_LEN];
    uint8_t buf_len;
    uint8_t blocks_compressed;
    uint8_t flags;
    uint32_t cv_stack[BLAKE3_MAX_DEPTH][8];
    uint8_t cv_stack_len;
    uint64_t chunk_counter;
} blake3_ctx;

/* Initialize with IV */
EXPORT void blake3_init(blake3_ctx* ctx) {
    memcpy(ctx->key, BLAKE3_IV, sizeof(BLAKE3_IV));
    memcpy(ctx->cv, BLAKE3_IV, sizeof(BLAKE3_IV));
    ctx->buf_len = 0;
    ctx->blocks_compressed = 0;
    ctx->flags = 0;
    ctx->cv_stack_len = 0;
    ctx->chunk_counter = 0;
}

/* Initialize with key */
EXPORT void blake3_init_keyed(blake3_ctx* ctx, const uint8_t key[32]) {
    for (int i = 0; i < 8; i++) {
        ctx->key[i] = (key[4*i]) | (key[4*i + 1] << 8) | 
                      (key[4*i + 2] << 16) | (key[4*i + 3] << 24);
    }
    memcpy(ctx->cv, ctx->key, sizeof(ctx->key));
    ctx->buf_len = 0;
    ctx->blocks_compressed = 0;
    ctx->flags = KEYED_HASH;
    ctx->cv_stack_len = 0;
    ctx->chunk_counter = 0;
}

/* Chunk state functions */
static uint8_t chunk_flags(blake3_ctx* ctx) {
    uint8_t flags = ctx->flags;
    if (ctx->blocks_compressed == 0) {
        flags |= CHUNK_START;
    }
    return flags;
}

static void output_chaining_value(blake3_ctx* ctx, uint32_t cv_out[8]) {
    uint32_t out[16];
    uint8_t flags = chunk_flags(ctx) | CHUNK_END;
    blake3_compress(ctx->cv, ctx->buf, ctx->buf_len, ctx->chunk_counter, flags, out);
    memcpy(cv_out, out, 8 * sizeof(uint32_t));
}

/* Push CV to stack */
static void push_cv(blake3_ctx* ctx, uint32_t cv[8]) {
    memcpy(ctx->cv_stack[ctx->cv_stack_len], cv, 8 * sizeof(uint32_t));
    ctx->cv_stack_len++;
}

/* Parent CV */
static void parent_cv(const uint32_t left[8], const uint32_t right[8], 
                      const uint32_t key[8], uint8_t flags, uint32_t out[8]) {
    uint8_t block[BLAKE3_BLOCK_LEN];
    
    for (int i = 0; i < 8; i++) {
        block[4*i]     = left[i] & 0xFF;
        block[4*i + 1] = (left[i] >> 8) & 0xFF;
        block[4*i + 2] = (left[i] >> 16) & 0xFF;
        block[4*i + 3] = (left[i] >> 24) & 0xFF;
    }
    for (int i = 0; i < 8; i++) {
        block[32 + 4*i]     = right[i] & 0xFF;
        block[32 + 4*i + 1] = (right[i] >> 8) & 0xFF;
        block[32 + 4*i + 2] = (right[i] >> 16) & 0xFF;
        block[32 + 4*i + 3] = (right[i] >> 24) & 0xFF;
    }
    
    uint32_t out16[16];
    blake3_compress(key, block, BLAKE3_BLOCK_LEN, 0, flags | PARENT, out16);
    memcpy(out, out16, 8 * sizeof(uint32_t));
}

/* Merge CVs */
static void add_chunk_cv(blake3_ctx* ctx, uint32_t cv[8], uint64_t total_chunks) {
    while ((total_chunks & 1) == 0 && ctx->cv_stack_len > 0) {
        ctx->cv_stack_len--;
        parent_cv(ctx->cv_stack[ctx->cv_stack_len], cv, ctx->key, ctx->flags, cv);
        total_chunks >>= 1;
    }
    push_cv(ctx, cv);
}

/* Update */
EXPORT void blake3_update(blake3_ctx* ctx, const uint8_t* input, size_t len) {
    while (len > 0) {
        /* Fill buffer */
        size_t take = BLAKE3_CHUNK_LEN - (ctx->blocks_compressed * BLAKE3_BLOCK_LEN + ctx->buf_len);
        if (take > len) take = len;
        
        if (ctx->buf_len + take <= BLAKE3_BLOCK_LEN) {
            memcpy(ctx->buf + ctx->buf_len, input, take);
            ctx->buf_len += take;
            input += take;
            len -= take;
            
            /* Process full block */
            if (ctx->buf_len == BLAKE3_BLOCK_LEN && len > 0) {
                uint32_t out[16];
                blake3_compress(ctx->cv, ctx->buf, BLAKE3_BLOCK_LEN, 
                               ctx->chunk_counter, chunk_flags(ctx), out);
                memcpy(ctx->cv, out, 8 * sizeof(uint32_t));
                ctx->blocks_compressed++;
                ctx->buf_len = 0;
            }
        } else {
            /* Process remaining */
            size_t want = BLAKE3_BLOCK_LEN - ctx->buf_len;
            memcpy(ctx->buf + ctx->buf_len, input, want);
            uint32_t out[16];
            blake3_compress(ctx->cv, ctx->buf, BLAKE3_BLOCK_LEN, 
                           ctx->chunk_counter, chunk_flags(ctx), out);
            memcpy(ctx->cv, out, 8 * sizeof(uint32_t));
            ctx->blocks_compressed++;
            ctx->buf_len = 0;
            input += want;
            len -= want;
            take -= want;
        }
        
        /* Chunk complete? */
        if (ctx->blocks_compressed == BLAKE3_CHUNK_LEN / BLAKE3_BLOCK_LEN) {
            uint32_t cv[8];
            output_chaining_value(ctx, cv);
            add_chunk_cv(ctx, cv, ctx->chunk_counter + 1);
            ctx->chunk_counter++;
            memcpy(ctx->cv, ctx->key, sizeof(ctx->key));
            ctx->blocks_compressed = 0;
            ctx->buf_len = 0;
        }
    }
}

/* Finalize */
EXPORT void blake3_finalize(blake3_ctx* ctx, uint8_t* out, size_t outlen) {
    /* Finalize current chunk */
    uint32_t cv[8];
    output_chaining_value(ctx, cv);
    
    /* Merge remaining CVs */
    while (ctx->cv_stack_len > 0) {
        ctx->cv_stack_len--;
        parent_cv(ctx->cv_stack[ctx->cv_stack_len], cv, ctx->key, ctx->flags, cv);
    }
    
    /* Root output */
    uint8_t root_block[BLAKE3_BLOCK_LEN] = {0};
    for (int i = 0; i < 8 && i * 4 < (int)outlen; i++) {
        root_block[4*i]     = cv[i] & 0xFF;
        root_block[4*i + 1] = (cv[i] >> 8) & 0xFF;
        root_block[4*i + 2] = (cv[i] >> 16) & 0xFF;
        root_block[4*i + 3] = (cv[i] >> 24) & 0xFF;
    }
    
    /* Output extension */
    size_t offset = 0;
    uint64_t counter = 0;
    
    while (offset < outlen) {
        uint32_t out16[16];
        blake3_compress(cv, root_block, BLAKE3_BLOCK_LEN, counter, ctx->flags | ROOT, out16);
        
        for (int i = 0; i < 16 && offset < outlen; i++) {
            for (int j = 0; j < 4 && offset < outlen; j++) {
                out[offset++] = (out16[i] >> (j * 8)) & 0xFF;
            }
        }
        counter++;
    }
}

/* Simple hash function */
EXPORT void blake3_hash(const uint8_t* input, size_t len, uint8_t* output) {
    blake3_ctx ctx;
    blake3_init(&ctx);
    blake3_update(&ctx, input, len);
    blake3_finalize(&ctx, output, 32);
}

/* Mining hash with nonce */
EXPORT void blake3_mine(
    const uint8_t* header,
    size_t header_len,
    uint64_t nonce,
    uint8_t* output
) {
    uint8_t data[256];
    memcpy(data, header, header_len);
    memcpy(data + header_len, &nonce, 8);
    
    blake3_hash(data, header_len + 8, output);
}

/* Alephium-style double Blake3 */
EXPORT void blake3_alph(
    const uint8_t* header,
    size_t header_len,
    uint64_t nonce,
    uint8_t* output
) {
    uint8_t temp[32];
    uint8_t data[256];
    
    memcpy(data, header, header_len);
    memcpy(data + header_len, &nonce, 8);
    
    /* First round */
    blake3_hash(data, header_len + 8, temp);
    
    /* Second round */
    blake3_hash(temp, 32, output);
}

/* Verify against target */
EXPORT int blake3_verify(
    const uint8_t* header,
    size_t header_len,
    uint64_t nonce,
    const uint8_t* target
) {
    uint8_t hash[32];
    blake3_mine(header, header_len, nonce, hash);
    
    for (int i = 31; i >= 0; i--) {
        if (hash[i] < target[i]) return 1;
        if (hash[i] > target[i]) return 0;
    }
    return 1;
}

/* Benchmark */
EXPORT double blake3_benchmark(int iterations) {
    uint8_t header[80] = {0x01, 0x02, 0x03};
    uint8_t output[32];
    
    clock_t start = clock();
    
    for (int i = 0; i < iterations; i++) {
        blake3_mine(header, 80, i, output);
    }
    
    clock_t end = clock();
    double seconds = (double)(end - start) / CLOCKS_PER_SEC;
    
    return iterations / seconds;
}

/* Test */
EXPORT void blake3_test() {
    printf("=== ZION Blake3 Native Library Test ===\n\n");
    
    /* Test vector */
    uint8_t input[] = "Hello, ZION!";
    uint8_t hash[32];
    
    blake3_hash(input, strlen((char*)input), hash);
    
    printf("Input: %s\n", input);
    printf("Hash:  ");
    for (int i = 0; i < 32; i++) printf("%02x", hash[i]);
    printf("\n\n");
    
    /* Mining test */
    uint8_t header[80] = {0x01, 0x02, 0x03, 0x04};
    blake3_mine(header, 80, 12345, hash);
    
    printf("Mining hash: ");
    for (int i = 0; i < 8; i++) printf("%02x", hash[i]);
    printf("...\n\n");
    
    printf("Benchmark (10000 iterations)...\n");
    double hashrate = blake3_benchmark(10000);
    printf("Hashrate: %.2f KH/s\n", hashrate / 1000);
}

EXPORT const char* blake3_version() {
    return "ZION Blake3 v1.0.0 - ALPH Compatible";
}

/* Main for testing */
#ifdef BLAKE3_TEST
int main() {
    blake3_test();
    return 0;
}
#endif
