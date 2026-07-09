/*
 * ZION Ekam Deeksha — Metal GPU Compute Shader
 * Apple Silicon M1–M5 Native Pipeline
 *
 * Exact match to Rust cosmic_harmony_ekam_deeksha() canonical hash:
 *   Step 1: Keccak-256 (header||nonce → 32 B)
 *   Step 2: SHA3-512 (32 B → 64 B)
 *   Step 3: Golden Matrix (φ^k fixed-point, 64 B → 64 B)
 *   Step 4: Memory-Hard (Blake3 XOF 256 KiB scratchpad, 4 passes, 256 reads)
 *   Step 5: NPU Mix (INT8 MLP 64→128→64 + residual)
 *   Step 6: Cosmic Fusion (8 × Keccak-256 + AES-128, final SHA3-512 → 32 B)
 *
 * Translated from canonical OpenCL kernel: cosmic_harmony_deeksha.cl
 *
 * Author: ZION AI Native Team
 * Version: 3.0.0-dev
 */

#include <metal_stdlib>
#include <metal_atomic>
using namespace metal;

// ============================================================================
// Constants
// ============================================================================

#define SCRATCHPAD_SIZE  262144
#define BLOCK_SIZE       64
#define BLOCK_COUNT      4096
#define PASSES           4
#define RANDOM_READS     256

#define ROL64(x, n) (((x) << (n)) | ((x) >> (64 - (n))))
#define XTIME(a) ((uchar)(((a) << 1) ^ ((((a) >> 7) & 1) * 0x1b)))

constant ulong RC[24] = {
    0x0000000000000001UL, 0x0000000000008082UL,
    0x800000000000808AUL, 0x8000000080008000UL,
    0x000000000000808BUL, 0x0000000080000001UL,
    0x8000000080008081UL, 0x8000000000008009UL,
    0x000000000000008AUL, 0x0000000000000088UL,
    0x0000000080008009UL, 0x000000008000000AUL,
    0x000000008000808BUL, 0x800000000000008BUL,
    0x8000000000008089UL, 0x8000000000008003UL,
    0x8000000000008002UL, 0x8000000000000080UL,
    0x000000000000800AUL, 0x800000008000000AUL,
    0x8000000080008081UL, 0x8000000000008080UL,
    0x0000000080000001UL, 0x8000000080008008UL,
};

constant ulong PHI_FP[16] = {
    4294967296UL,     6949403065UL,     11244370361UL,    18193773427UL,
    29438143788UL,    47631917215UL,    77070061004UL,    124701978219UL,
    201772039223UL,   326474017443UL,   528246056666UL,   854720074109UL,
    1382966130776UL,  2237686204885UL,  3620652335660UL,  5858338540545UL,
};

constant uchar AES_SBOX[256] = {
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

constant uchar AES_RCON[10] = {
    0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36
};

constant uint BLAKE3_IV[8] = {
    0x6A09E667u, 0xBB67AE85u, 0x3C6EF372u, 0xA54FF53Au,
    0x510E527Fu, 0x9B05688Cu, 0x1F83D9ABu, 0x5BE0CD19u
};

constant uchar BLAKE3_MSG_PERM[16] = {
    2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8
};

#define B3_CHUNK_START 1u
#define B3_CHUNK_END   2u
#define B3_ROOT        8u

constant uchar EKAM_DOMAIN_SEP[23] = {
    'E','K','A','M','_','S','C','R','A','T','C','H','P','A','D','_','I','N','I','T','_','V','1'
};

// ============================================================================
// Keccak-f1600
// ============================================================================

#define CHI_ROW(b) \
{ ulong _a=st[(b)],_b=st[(b)+1],_c=st[(b)+2],_d=st[(b)+3],_e=st[(b)+4]; \
  st[(b)]    = _a ^ ((~_b) & _c); \
  st[(b)+1]  = _b ^ ((~_c) & _d); \
  st[(b)+2]  = _c ^ ((~_d) & _e); \
  st[(b)+3]  = _d ^ ((~_e) & _a); \
  st[(b)+4]  = _e ^ ((~_a) & _b); }

inline void keccak_f1600(thread ulong *st)
{
    ulong bc0, bc1, bc2, bc3, bc4, t;

    for (int rnd = 0; rnd < 24; rnd++) {
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

        CHI_ROW(0) CHI_ROW(5) CHI_ROW(10) CHI_ROW(15) CHI_ROW(20)

        st[0] ^= RC[rnd];
    }
}

// ============================================================================
// Keccak absorb / finalize
// ============================================================================

inline void keccak_absorb(thread ulong *st, thread int *pos, int rate,
                          thread const uchar *in, int inlen)
{
    while (inlen > 0) {
        int chunk = rate - *pos;
        if (chunk > inlen) chunk = inlen;
        int off = *pos;
        int i = 0;
        if ((off & 7) == 0) {
            int ulongs = chunk >> 3;
            for (int u = 0; u < ulongs; u++) {
                ulong v = 0;
                for (int b = 0; b < 8; b++)
                    v |= (ulong)in[u * 8 + b] << (b * 8);
                st[(off >> 3) + u] ^= v;
            }
            i = ulongs << 3;
        }
        for (; i < chunk; i++)
            ((thread uchar*)st)[off + i] ^= in[i];
        in    += chunk;
        inlen -= chunk;
        *pos  += chunk;
        if (*pos == rate) {
            keccak_f1600(st);
            *pos = 0;
        }
    }
}

inline void keccak_finalize(thread ulong *st, int pos, int rate,
                            uchar pad_byte, thread uchar *out, int outlen)
{
    ((thread uchar*)st)[pos]      ^= pad_byte;
    ((thread uchar*)st)[rate - 1] ^= 0x80;
    keccak_f1600(st);
    int ulongs = outlen >> 3;
    thread ulong *out64 = (thread ulong *)out;
    for (int i = 0; i < ulongs; i++) out64[i] = st[i];
    for (int i = (ulongs << 3); i < outlen; i++)
        out[i] = ((thread uchar*)st)[i];
}

inline void keccak256(thread const uchar *in, int inlen, thread uchar *out)
{
    thread ulong st[25]; int pos = 0;
    for (int i = 0; i < 25; i++) st[i] = 0;
    keccak_absorb(st, &pos, 136, in, inlen);
    keccak_finalize(st, pos, 136, 0x01, out, 32);
}

inline void keccak256_136_mix(thread const ulong *acc64, thread const ulong *chunk64,
                              ulong r_val, thread ulong *out64)
{
    thread ulong st[25];
    for (int i = 0; i < 25; i++) st[i] = 0;
    for (int i = 0; i < 8; i++) st[i] = acc64[i];
    for (int i = 0; i < 8; i++) st[8 + i] = chunk64[i];
    st[16] = r_val;
    keccak_f1600(st);
    st[0]  ^= 0x01UL;
    st[16] ^= 0x8000000000000000UL;
    keccak_f1600(st);
    for (int i = 0; i < 4; i++) out64[i] = st[i];
}

inline void sha3_512(thread const uchar *in, int inlen, thread uchar *out)
{
    thread ulong st[25]; int pos = 0;
    for (int i = 0; i < 25; i++) st[i] = 0;
    keccak_absorb(st, &pos, 72, in, inlen);
    keccak_finalize(st, pos, 72, 0x06, out, 64);
}

// ============================================================================
// Step 3: Golden Matrix (64 B → 64 B)
// ============================================================================

inline void golden_matrix(thread const uchar *in64, thread uchar *out64)
{
    ulong result[8];
    for (int i = 0; i < 8; i++) {
        ulong sum = 0;
        for (int j = 0; j < 8; j++)
            sum += (ulong)in64[i * 8 + j] * PHI_FP[i + j];
        result[i] = sum >> 32;
    }
    for (int i = 0; i < 8; i++) {
        ulong v = result[i];
        for (int b = 0; b < 8; b++)
            out64[i * 8 + b] = (uchar)(v >> (b * 8));
    }
}

// ============================================================================
// BLAKE3 Engine
// ============================================================================

inline uint b3_rotr32(uint x, int n) {
    return (x >> n) | (x << (32 - n));
}

inline void b3_g(thread uint *st, int a, int b, int c, int d, uint mx, uint my) {
    st[a] = st[a] + st[b] + mx;
    st[d] = b3_rotr32(st[d] ^ st[a], 16);
    st[c] = st[c] + st[d];
    st[b] = b3_rotr32(st[b] ^ st[c], 12);
    st[a] = st[a] + st[b] + my;
    st[d] = b3_rotr32(st[d] ^ st[a], 8);
    st[c] = st[c] + st[d];
    st[b] = b3_rotr32(st[b] ^ st[c], 7);
}

inline void b3_round(thread uint *st, thread const uint *msg) {
    b3_g(st, 0, 4,  8, 12, msg[0],  msg[1]);
    b3_g(st, 1, 5,  9, 13, msg[2],  msg[3]);
    b3_g(st, 2, 6, 10, 14, msg[4],  msg[5]);
    b3_g(st, 3, 7, 11, 15, msg[6],  msg[7]);
    b3_g(st, 0, 5, 10, 15, msg[8],  msg[9]);
    b3_g(st, 1, 6, 11, 12, msg[10], msg[11]);
    b3_g(st, 2, 7,  8, 13, msg[12], msg[13]);
    b3_g(st, 3, 4,  9, 14, msg[14], msg[15]);
}

inline void b3_permute(thread uint *msg) {
    uint tmp[16];
    for (int i = 0; i < 16; i++) tmp[i] = msg[BLAKE3_MSG_PERM[i]];
    for (int i = 0; i < 16; i++) msg[i] = tmp[i];
}

inline void b3_compress(thread const uint *cv, thread const uint *bw,
                        ulong counter, uint block_len, uint flags,
                        thread uint *output)
{
    uint st[16] = {
        cv[0], cv[1], cv[2], cv[3],
        cv[4], cv[5], cv[6], cv[7],
        BLAKE3_IV[0], BLAKE3_IV[1], BLAKE3_IV[2], BLAKE3_IV[3],
        (uint)(counter & 0xFFFFFFFFu),
        (uint)(counter >> 32),
        block_len,
        flags
    };
    uint msg[16];
    for (int i = 0; i < 16; i++) msg[i] = bw[i];
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

inline void b3_compress_cv(thread const uint *cv, thread const uint *bw,
                           ulong counter, uint block_len, uint flags,
                           thread uint *out_cv)
{
    uint full[16];
    b3_compress(cv, bw, counter, block_len, flags, full);
    for (int i = 0; i < 8; i++) out_cv[i] = full[i];
}

inline void b3_load_words(thread const uchar *buf, int len, thread uint *words) {
    for (int i = 0; i < 16; i++) words[i] = 0;
    for (int i = 0; i < len; i++)
        words[i / 4] |= (uint)buf[i] << ((i % 4) * 8);
}

inline void b3_load_words_global(device const uchar *buf, int len, thread uint *words) {
    device const uint *buf32 = (device const uint *)buf;
    if (len >= 64) {
        // Fast path: 64 bytes = 16 words, no loop overhead.
        words[0]  = buf32[0];  words[1]  = buf32[1];  words[2]  = buf32[2];  words[3]  = buf32[3];
        words[4]  = buf32[4];  words[5]  = buf32[5];  words[6]  = buf32[6];  words[7]  = buf32[7];
        words[8]  = buf32[8];  words[9]  = buf32[9];  words[10] = buf32[10]; words[11] = buf32[11];
        words[12] = buf32[12]; words[13] = buf32[13]; words[14] = buf32[14]; words[15] = buf32[15];
    } else {
        int wcount = len >> 2;
        for (int i = 0; i < wcount; i++) words[i] = buf32[i];
        for (int i = wcount; i < 16; i++) words[i] = 0;
        int done = wcount << 2;
        if (done < len) {
            uint w = 0;
            for (int i = done; i < len; i++)
                w |= (uint)buf[i] << ((i - done) * 8);
            words[wcount] = w;
        }
    }
}

struct B3ChunkOut {
    uint input_cv[8];
    uint block_words[16];
    uint block_len;
    uint flags;
};

inline B3ChunkOut b3_hash_single_chunk(thread const uchar *input, uint input_len) {
    B3ChunkOut out;
    uint cv[8];
    for (int i = 0; i < 8; i++) cv[i] = BLAKE3_IV[i];
    uint offset = 0;
    while (offset < input_len) {
        uint remaining = input_len - offset;
        uint this_len = (remaining > 64u) ? 64u : remaining;
        int is_first = (offset == 0);
        int is_last  = (offset + this_len >= input_len);
        uint fl = 0u;
        if (is_first) fl |= B3_CHUNK_START;
        if (is_last)  fl |= B3_CHUNK_END;
        uint bw[16];
        b3_load_words(input + offset, (int)this_len, bw);
        if (is_last) {
            for (int i = 0; i < 8; i++) out.input_cv[i] = cv[i];
            for (int i = 0; i < 16; i++) out.block_words[i] = bw[i];
            out.block_len = this_len;
            out.flags = fl;
            return out;
        }
        b3_compress_cv(cv, bw, 0UL, this_len, fl, cv);
        offset += this_len;
    }
    for (int i = 0; i < 8; i++) out.input_cv[i] = BLAKE3_IV[i];
    for (int i = 0; i < 16; i++) out.block_words[i] = 0;
    out.block_len = 0;
    out.flags = B3_CHUNK_START | B3_CHUNK_END;
    return out;
}

inline void b3_xof_fill_global(B3ChunkOut co, device uchar *buf, uint buf_len) {
    device uint *buf32 = (device uint *)buf;
    uint ob = 0, written = 0;
    while (written < buf_len) {
        uint st[16];
        b3_compress(co.input_cv, co.block_words, (ulong)ob,
                    co.block_len, co.flags | B3_ROOT, st);
        uint to_write = min(64u, buf_len - written);
        uint words = to_write >> 2;
        uint base = written >> 2;
        for (uint i = 0; i < words; i++)
            buf32[base + i] = st[i];
        uint done = words << 2;
        for (uint i = done; i < to_write; i++)
            buf[written + i] = (uchar)(st[i / 4] >> ((i % 4) * 8));
        written += to_write;
        ob++;
    }
}

inline void b3_xof_fill_private(B3ChunkOut co, thread uchar *buf, uint buf_len) {
    uint ob = 0, written = 0;
    while (written < buf_len) {
        uint st[16];
        b3_compress(co.input_cv, co.block_words, (ulong)ob,
                    co.block_len, co.flags | B3_ROOT, st);
        uint to_write = min(64u, buf_len - written);
        uint full_words = to_write >> 2;
        thread uint *dst32 = (thread uint *)(buf + written);
        for (uint w = 0; w < full_words; w++) dst32[w] = st[w];
        for (uint i = (full_words << 2); i < to_write; i++)
            buf[written + i] = (uchar)(st[i / 4] >> ((i % 4) * 8));
        written += to_write;
        ob++;
    }
}

// ============================================================================
// Step 4: Ekam Memory-Hard (Blake3 XOF scratchpad)
// ============================================================================

inline void ekam_init_scratchpad(thread const uchar *seed, device uchar *pad)
{
    uchar input[87];
    for (int i = 0; i < 64; i++) input[i] = seed[i];
    for (int i = 0; i < 23; i++) input[64 + i] = EKAM_DOMAIN_SEP[i];
    B3ChunkOut co = b3_hash_single_chunk(input, 87u);
    b3_xof_fill_global(co, pad, SCRATCHPAD_SIZE);
}

inline void ekam_mix_block(device uchar *pad, uint index, ulong pass, int forward)
{
    uint prev_index;
    if (forward)
        prev_index = (index == 0) ? (BLOCK_COUNT - 1) : (index - 1);
    else
        prev_index = (index + 1 == BLOCK_COUNT) ? 0 : (index + 1);

    uint cur_off  = index * BLOCK_SIZE;
    uint prev_off = prev_index * BLOCK_SIZE;

    ulong idx_val = *((device const ulong *)(pad + cur_off));
    uint rand_index = (uint)((idx_val ^ pass ^ (ulong)index) % BLOCK_COUNT);
    uint rand_off = rand_index * BLOCK_SIZE;

    uint cv[8];
    for (int i = 0; i < 8; i++) cv[i] = BLAKE3_IV[i];
    uint bw[16];

    b3_load_words_global(pad + cur_off, 64, bw);
    b3_compress_cv(cv, bw, 0UL, 64, B3_CHUNK_START, cv);

    b3_load_words_global(pad + prev_off, 64, bw);
    b3_compress_cv(cv, bw, 0UL, 64, 0, cv);

    b3_load_words_global(pad + rand_off, 64, bw);
    b3_compress_cv(cv, bw, 0UL, 64, 0, cv);

    for (int i = 0; i < 16; i++) bw[i] = 0;
    bw[0] = (uint)(pass & 0xFFFFFFFFu);
    bw[1] = (uint)(pass >> 32);
    bw[2] = (uint)((ulong)index & 0xFFFFFFFFu);
    bw[3] = (uint)((ulong)index >> 32);

    B3ChunkOut co;
    for (int i = 0; i < 8; i++) co.input_cv[i] = cv[i];
    for (int i = 0; i < 16; i++) co.block_words[i] = bw[i];
    co.block_len = 16;
    co.flags = B3_CHUNK_END;

    uchar mixed[64];
    b3_xof_fill_private(co, mixed, 64u);

    device ulong *dst = (device ulong *)(pad + cur_off);
    thread ulong *src = (thread ulong *)mixed;
    for (int i = 0; i < 8; i++)
        dst[i] ^= src[i];
}

inline void ekam_sequential_passes(device uchar *pad)
{
    for (int pass = 0; pass < PASSES; pass++) {
        int forward = (pass % 2 == 0);
        if (forward) {
            for (uint i = 0; i < BLOCK_COUNT; i++)
                ekam_mix_block(pad, i, (ulong)pass, 1);
        } else {
            for (int i = BLOCK_COUNT - 1; i >= 0; i--)
                ekam_mix_block(pad, (uint)i, (ulong)pass, 0);
        }
    }
}

inline void random_read_mix(thread const uchar *seed, device const uchar *pad,
                            thread uchar *out)
{
    uchar acc[64];
    { thread ulong *d = (thread ulong *)acc; thread const ulong *s = (thread const ulong *)seed;
      for (int i = 0; i < 8; i++) d[i] = s[i]; }

    ulong pos_val = *(thread const ulong *)seed;
    uint pos = (uint)(pos_val % BLOCK_COUNT);

    for (int r = 0; r < RANDOM_READS; r++) {
        uint off = pos * BLOCK_SIZE;

        device const ulong *gsrc = (device const ulong *)(pad + off);
        ulong chunk64[8];
        for (int i = 0; i < 8; i++) chunk64[i] = gsrc[i];

        ulong d64[4];
        keccak256_136_mix((thread const ulong *)acc, chunk64, (ulong)r, d64);

        {
            thread ulong *a64 = (thread ulong *)acc;
            for (int u = 0; u < 4; u++) a64[u] ^= d64[u];
        }
        {
            thread const uchar *d8 = (thread const uchar *)d64;
            for (int i = 0; i < 32; i++)
                acc[32 + i] = (uchar)((uint)acc[32 + i] + (uint)d8[i]);
        }

        pos = (uint)((d64[0] ^ (ulong)pos ^ (ulong)r) % BLOCK_COUNT);
    }

    uchar first_blk[BLOCK_SIZE], last_blk[BLOCK_SIZE];
    {
        device const ulong *fp = (device const ulong *)pad;
        device const ulong *lp = (device const ulong *)(pad + SCRATCHPAD_SIZE - BLOCK_SIZE);
        thread ulong *fd = (thread ulong *)first_blk;
        thread ulong *ld = (thread ulong *)last_blk;
        for (int i = 0; i < 8; i++) {
            fd[i] = fp[i];
            ld[i] = lp[i];
        }
    }

    ulong fst[25]; int fpos = 0;
    for (int i = 0; i < 25; i++) fst[i] = 0;
    keccak_absorb(fst, &fpos, 72, acc,       64);
    keccak_absorb(fst, &fpos, 72, first_blk, BLOCK_SIZE);
    keccak_absorb(fst, &fpos, 72, last_blk,  BLOCK_SIZE);
    keccak_finalize(fst, fpos, 72, 0x06, out, 64);
}

inline void ekam_memory_hard_transform(thread const uchar *input, device uchar *pad,
                                       thread uchar *output)
{
    ekam_init_scratchpad(input, pad);
    ekam_sequential_passes(pad);
    random_read_mix(input, pad, output);
}

// ============================================================================
// Step 5: NPU Mix — variable-topology INT8 MLP + residual
//
// Supports all 4 epoch topologies:
//   Standard:   64→128→64  (2 layers)
//   ThreeLayer: 64→96→128→64 (3 layers)
//   Wide:       64→256→64  (2 layers)
//   Deep:       64→64→64→64 (3 layers)
//
// npu_meta layout: [num_layers, in0, out0, in1, out1, in2, out2]
// All weights/biases/scales packed sequentially per layer.
// ============================================================================

inline int gelu_int8(int x)
{
    int num = x * (128 + x);
    int result = num >> 8;
    return clamp(result, -128, 127);
}

inline void npu_mix(thread const uchar *in64, thread uchar *out64,
                    device const char *weights_all,
                    device const char *biases_all,
                    device const short *scales_all,
                    device const uint *npu_meta)
{
    uint num_layers = npu_meta[0];

    // Working buffers — max dimension 256 (Wide topology)
    int current[256];
    int next_buf[256];

    // Convert input u8 → i32 (signed reinterpret)
    for (int i = 0; i < 64; i++)
        current[i] = (int)((char)in64[i]);

    // Save residual (input is always 64)
    int residual[64];
    for (int i = 0; i < 64; i++)
        residual[i] = current[i];

    uint w_offset = 0;
    uint b_offset = 0;
    uint s_offset = 0;

    for (uint layer = 0; layer < num_layers; layer++) {
        uint in_dim  = npu_meta[1 + layer * 2];
        uint out_dim = npu_meta[2 + layer * 2];

        // MatMul + bias
        for (uint i = 0; i < out_dim; i++) {
            int acc = (int)biases_all[b_offset + i] * 32;
            for (uint j = 0; j < in_dim; j++)
                acc += current[j] * (int)weights_all[w_offset + i * in_dim + j];
            next_buf[i] = clamp(acc >> 12, -128, 127);
        }

        // LayerNorm
        {
            long sum = 0;
            for (uint i = 0; i < out_dim; i++) sum += (long)next_buf[i];
            int mean = (int)(sum / (long)out_dim);
            long var_sum = 0;
            for (uint i = 0; i < out_dim; i++) {
                long d = (long)(next_buf[i] - mean);
                var_sum += d * d;
            }
            uint var_u = (uint)(var_sum / (long)out_dim);
            uint std_approx = 1;
            if (var_u > 0) {
                std_approx = (uint)sqrt((float)var_u) + 1;
            }
            for (uint i = 0; i < out_dim; i++) {
                int normalized = ((next_buf[i] - mean) * 128) / (int)std_approx;
                next_buf[i] = clamp((normalized * (int)scales_all[s_offset + i]) >> 8, -128, 127);
            }
        }

        // GELU for all but last layer
        if (layer < num_layers - 1) {
            for (uint i = 0; i < out_dim; i++)
                next_buf[i] = gelu_int8(next_buf[i]);
        }

        // Advance: next → current
        for (uint i = 0; i < out_dim; i++)
            current[i] = next_buf[i];

        w_offset += out_dim * in_dim;
        b_offset += out_dim;
        s_offset += out_dim;
    }

    // Residual add + convert i32 → u8
    for (int i = 0; i < 64; i++) {
        int v = clamp(current[i] + residual[i], -128, 127);
        out64[i] = (uchar)v;
    }
}

// ============================================================================
// AES-128 (FIPS 197)
// ============================================================================

inline void aes_shift_rows(thread uchar *s)
{
    uchar t;
    t = s[1]; s[1] = s[5]; s[5] = s[9]; s[9] = s[13]; s[13] = t;
    t = s[2]; s[2] = s[10]; s[10] = t;
    t = s[6]; s[6] = s[14]; s[14] = t;
    t = s[15]; s[15] = s[11]; s[11] = s[7]; s[7] = s[3]; s[3] = t;
}

inline void aes_mix_columns(thread uchar *s)
{
    for (int c = 0; c < 4; c++) {
        int off = c * 4;
        uchar a0 = s[off], a1 = s[off+1], a2 = s[off+2], a3 = s[off+3];
        s[off]   = XTIME(a0) ^ XTIME(a1) ^ a1 ^ a2 ^ a3;
        s[off+1] = a0 ^ XTIME(a1) ^ XTIME(a2) ^ a2 ^ a3;
        s[off+2] = a0 ^ a1 ^ XTIME(a2) ^ XTIME(a3) ^ a3;
        s[off+3] = XTIME(a0) ^ a0 ^ a1 ^ a2 ^ XTIME(a3);
    }
}

inline void aes128_encrypt(thread const uchar *key, thread uchar *block)
{
    uchar rk[176];
    for (int i = 0; i < 16; i++) rk[i] = key[i];

    for (int i = 16; i < 176; i += 4) {
        uchar t0 = rk[i-4], t1 = rk[i-3], t2 = rk[i-2], t3 = rk[i-1];
        if ((i & 15) == 0) {
            uchar tmp = t0;
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

// ============================================================================
// Step 6: Cosmic Fusion (8 rounds)
// ============================================================================

inline void fusion_round(thread uchar *state, uchar round_num)
{
    uchar hash_input[33];
    {
        thread ulong *hi64 = (thread ulong *)hash_input;
        thread ulong *st64 = (thread ulong *)state;
        for (int i = 0; i < 4; i++) hi64[i] = st64[i];
    }
    hash_input[32] = round_num;

    uchar intermediate[32];
    keccak256(hash_input, 33, intermediate);

    uchar aes_key[16], block0[16], block1[16];
    {
        thread ulong *k64 = (thread ulong *)aes_key;
        thread ulong *i64 = (thread ulong *)intermediate;
        k64[0] = i64[0]; k64[1] = i64[1];
    }
    {
        thread ulong *b64 = (thread ulong *)block0;
        thread ulong *s64 = (thread ulong *)(state + 32);
        b64[0] = s64[0]; b64[1] = s64[1];
    }
    aes128_encrypt(aes_key, block0);

    uchar key2[16];
    {
        thread ulong *k264 = (thread ulong *)key2;
        thread ulong *k64  = (thread ulong *)aes_key;
        k264[0] = k64[0]; k264[1] = k64[1];
    }
    key2[0]  ^= round_num;
    key2[15] ^= 0xAB;
    {
        thread ulong *b64 = (thread ulong *)block1;
        thread ulong *s64 = (thread ulong *)(state + 48);
        b64[0] = s64[0]; b64[1] = s64[1];
    }
    aes128_encrypt(key2, block1);

    uchar mask[32];
    {
        thread ulong *m64 = (thread ulong *)mask;
        thread ulong *b064 = (thread ulong *)block0;
        thread ulong *b164 = (thread ulong *)block1;
        m64[0] = b064[0]; m64[1] = b064[1];
        m64[2] = b164[0]; m64[3] = b164[1];
    }

    {
        thread ulong *s64 = (thread ulong *)(state + 32);
        thread ulong *i64 = (thread ulong *)intermediate;
        for (int i = 0; i < 4; i++) s64[i] ^= i64[i];
    }

    {
        thread ulong *s64 = (thread ulong *)state;
        thread ulong *i64 = (thread ulong *)intermediate;
        thread ulong *m64 = (thread ulong *)mask;
        for (int i = 0; i < 4; i++) s64[i] = i64[i] ^ m64[i];
    }
}

inline void cosmic_fusion_ekam(thread const uchar *in64, thread uchar *hash32)
{
    uchar state[64];
    { thread ulong *d = (thread ulong *)state; thread const ulong *s = (thread const ulong *)in64;
      for (int i = 0; i < 8; i++) d[i] = s[i]; }

    for (uchar r = 0; r < 8; r++)
        fusion_round(state, r);

    uchar full[64];
    sha3_512(state, 32, full);
    { thread ulong *d = (thread ulong *)hash32; thread ulong *s = (thread ulong *)full;
      for (int i = 0; i < 4; i++) d[i] = s[i]; }
}

// ============================================================================
// Main Kernel: ekam_deeksha_mine
// ============================================================================

kernel void ekam_deeksha_mine(
    device const uchar   *header          [[ buffer(0)  ]],
    device const uint    *params          [[ buffer(1)  ]],  // [header_len, nonce_count, target_u32]
    device const ulong   *nonce_base_buf  [[ buffer(2)  ]],
    device       uchar   *scratchpad_pool [[ buffer(3)  ]],
    device atomic_uint   *result_flag     [[ buffer(4)  ]],  // [0]=flag, [1]=nonce_lo, [2]=nonce_hi
    device       uchar   *result_hash     [[ buffer(5)  ]],
    device const char    *npu_weights     [[ buffer(6)  ]],
    device const char    *npu_biases      [[ buffer(7)  ]],
    device const short   *npu_scales      [[ buffer(8)  ]],
    device const uint    *npu_meta        [[ buffer(9)  ]],
    uint                  gid             [[ thread_position_in_grid ]]
)
{
    uint header_len  = params[0];
    uint nonce_count = params[1];
    uint target_u32  = params[2];
    ulong nonce_base = nonce_base_buf[0];

    if (gid >= nonce_count) return;

    ulong nonce = nonce_base + (ulong)gid;
    device uchar *pad = scratchpad_pool + (ulong)gid * SCRATCHPAD_SIZE;

    // Build input: header (<=80 B) + nonce (8 B LE) = 88 B
    uchar input[88];
    thread ulong *inp64 = (thread ulong *)input;
    for (int i = 0; i < 11; i++) inp64[i] = 0;
    uint hlen = min(header_len, (uint)80);
    device const uint *hdr32 = (device const uint *)header;
    thread uint *inp32 = (thread uint *)input;
    uint hwords = hlen >> 2;
    for (uint i = 0; i < hwords; i++) inp32[i] = hdr32[i];
    for (uint i = (hwords << 2); i < hlen; i++) input[i] = header[i];
    inp64[10] = nonce;

    uchar s1[32];
    keccak256(input, 88, s1);

    uchar s2[64];
    sha3_512(s1, 32, s2);

    uchar s3[64];
    golden_matrix(s2, s3);

    uchar s4[64];
    ekam_memory_hard_transform(s3, pad, s4);

    uchar s5[64];
    npu_mix(s4, s5, npu_weights, npu_biases, npu_scales, npu_meta);

    uchar hash[32];
    cosmic_fusion_ekam(s5, hash);

    // Compare first 4 bytes of hash vs target as big-endian u32 (PoW convention)
    uint state0 = ((uint)hash[0] << 24) | ((uint)hash[1] << 16) | ((uint)hash[2] << 8) | (uint)hash[3];

    if (state0 <= target_u32) {
        // Use 32-bit atomic exchange as found-flag (M1 Metal lacks 64-bit CAS)
        uint old = atomic_exchange_explicit(&result_flag[0], 0u, memory_order_relaxed);
        if (old == 0xFFFFFFFFu) {
            // We won the race — write nonce as two u32 and the hash
            atomic_store_explicit(&result_flag[1], (uint)(nonce & 0xFFFFFFFFu), memory_order_relaxed);
            atomic_store_explicit(&result_flag[2], (uint)(nonce >> 32), memory_order_relaxed);
            device uint *rh32 = (device uint *)result_hash;
            thread uint *h32 = (thread uint *)hash;
            for (int i = 0; i < 8; i++)
                rh32[i] = h32[i];
        }
    }
}
