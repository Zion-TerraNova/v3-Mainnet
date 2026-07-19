/*
 * DeekshaLite Fire — OpenCL Kernel (GCN/RDNA compatible)
 *
 * Fire = DeekshaLite v1 (identical, verified working) + thermal_loop step.
 * The thermal loop burns ALU cycles after the AES mix to maximize GPU heat.
 * All hashing / memory logic is 100% identical to deeksha_lite.cl (v1).
 *
 * Pipeline:
 *   1. Keccak256(header || nonce)  — host precomputed state (same as v1)
 *   2. Memory-hard scratchpad (256 KiB, 8192 blocks, 2 passes, 64 reads)
 *   3. AES-128 CTR mix (3 full rounds + 1 final)
 *   4. Thermal loop (16384 iters, 8 ulong chains) — extra heat, no float
 *   5. Keccak256(s3_after_thermal) → final hash
 *
 * Compatible with: AMD GCN (Vega, Polaris), AMD RDNA, NVIDIA, Intel.
 */

/* cl_khr_int64_base_atomics NOT needed — disabled to avoid compiler issues on gfx900. */

/* ========================================================================== */
/* Constants — identical to v1 for memory management                          */
/* ========================================================================== */

#define SCRATCHPAD_SIZE  262144   /* 256 KiB = 8192 * 32 — same as v1 */
#define BLOCK_SIZE       32
#define BLOCK_COUNT      8192
#define PASSES           2
#define RANDOM_READS     64
#define THERMAL_ITERS    16384 // OPTIMIZED: reduced from 65536 (4x less) for better efficiency

/* ========================================================================== */
/* Keccak — identical to deeksha_lite.cl                                      */
/* ========================================================================== */

/* AMD Vega/GCN/RDNA: use rotate(long,long) — maps to v_alignbyte on GCN.
 * The bit-shift version was slower by ~115ms/batch on Vega 64 (i066d). */
#define ROL64(x, n) rotate((long)((ulong)(x)), (long)((ulong)(n)))

#define CHI_ROW(b) \
{ ulong _a=st[(b)],_b=st[(b)+1],_c=st[(b)+2],_d=st[(b)+3],_e=st[(b)+4]; \
  st[(b)]   = _a ^ ((~_b) & _c); \
  st[(b)+1] = _b ^ ((~_c) & _d); \
  st[(b)+2] = _c ^ ((~_d) & _e); \
  st[(b)+3] = _d ^ ((~_e) & _a); \
  st[(b)+4] = _e ^ ((~_a) & _b); }

__constant ulong KC_RC[24] = {
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

__attribute__((always_inline))
void keccak_f1600(__private ulong *st)
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
        st[0] ^= KC_RC[rnd];
    }
}

typedef union { ulong u[25]; uchar b[200]; } keccak_st_t;

/* Same as v1: host precomputes keccak state after absorbing header[0..80] */
void keccak256_from_state(
    __global const ulong *pre_state,
    ulong nonce,
    __private uchar out[32])
{
    /* u64-optimized: matches CUDA keccak256_from_state.
     * Avoids byte-by-byte nonce XOR (8 ops → 1 op). */
    __private ulong st[25];
    for (int i = 0; i < 25; i++) st[i] = pre_state[i];
    /* XOR nonce into bytes 80..87 = st[10] */
    st[10] ^= nonce;
    /* Pad: 0x01 at byte 88 (st[11] byte 0), 0x80 at byte 135 (st[16] byte 7) */
    st[11] ^= 0x01UL;
    st[16] ^= (0x80UL << 56);
    keccak_f1600(st);
    /* Output first 32 bytes = 4 u64s */
    __private ulong *out_u64 = (__private ulong*)out;
    out_u64[0] = st[0]; out_u64[1] = st[1]; out_u64[2] = st[2]; out_u64[3] = st[3];
}

void sha3_512(__private const uchar *in, uint inlen, __private uchar out[64])
{
    keccak_st_t s;
    for (int i = 0; i < 25; i++) s.u[i] = 0;
    uint pos = 0;
    for (uint i = 0; i < inlen; i++) {
        s.b[pos] ^= in[i];
        if (++pos == 72) { keccak_f1600(s.u); pos = 0; }
    }
    s.b[pos] ^= 0x06;
    s.b[71]  ^= 0x80;
    keccak_f1600(s.u);
    for (int i = 0; i < 64; i++) out[i] = s.b[i];
}

/* SHA3-512 specialized for 65-byte input (used by fill_scratchpad).
 * Input always fits in one keccak block (rate=72 > 65), so no
 * mid-absorption permutation needed. Eliminates 65 conditional
 * branches per call and vectorizes the state zero + copy.
 *
 * u64-optimized version: takes 8 u64s of state + 1 byte block index,
 * outputs 8 u64s. Matches CUDA sha3_512_65_u64 — avoids 65 byte-by-byte
 * XOR operations that cause high register pressure on AMD. */
inline void sha3_512_65_u64(
    __private const ulong state_in[8],
    uchar blk_byte,
    __private ulong out_u64[8])
{
    __private ulong st[25];
    st[0]=state_in[0]; st[1]=state_in[1]; st[2]=state_in[2]; st[3]=state_in[3];
    st[4]=state_in[4]; st[5]=state_in[5]; st[6]=state_in[6]; st[7]=state_in[7];
    st[8]=0; st[9]=0; st[10]=0; st[11]=0; st[12]=0; st[13]=0; st[14]=0; st[15]=0;
    st[16]=0; st[17]=0; st[18]=0; st[19]=0; st[20]=0; st[21]=0; st[22]=0; st[23]=0;
    st[24]=0;

    /* XOR byte 64 into low byte of st[8] */
    st[8] ^= (ulong)blk_byte;
    /* Pad: 0x06 at byte 65 (st[8] byte 1), 0x80 at byte 71 (st[8] byte 7) */
    st[8] ^= (0x06UL << 8) | (0x80UL << 56);

    keccak_f1600(st);

    out_u64[0]=st[0]; out_u64[1]=st[1]; out_u64[2]=st[2]; out_u64[3]=st[3];
    out_u64[4]=st[4]; out_u64[5]=st[5]; out_u64[6]=st[6]; out_u64[7]=st[7];
}


/* ========================================================================== */
/* AES-128 helpers — identical to v1                                          */
/* ========================================================================== */

__constant uchar AES_SBOX[256] = {
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

void aes_sub_bytes(__private uchar s[16])   { for (int i=0;i<16;i++) s[i]=AES_SBOX[s[i]]; }

void aes_shift_rows(__private uchar s[16])
{
    uchar t;
    t=s[1];  s[1] =s[5];  s[5] =s[9];  s[9] =s[13]; s[13]=t;
    t=s[2];  s[2] =s[10]; s[10]=t;
    t=s[6];  s[6] =s[14]; s[14]=t;
    t=s[15]; s[15]=s[11]; s[11]=s[7];  s[7] =s[3];  s[3] =t;
}

uchar aes_xtime(uchar a) { return (uchar)((a<<1)^(((a>>7)&1)*0x1b)); }

void aes_mix_columns(__private uchar s[16])
{
    for (int i=0;i<4;i++) {
        uchar a=s[i*4],b=s[i*4+1],c=s[i*4+2],d=s[i*4+3];
        uchar e=a^b^c^d;
        s[i*4]  ^=e^aes_xtime(a^b); s[i*4+1]^=e^aes_xtime(b^c);
        s[i*4+2]^=e^aes_xtime(c^d); s[i*4+3]^=e^aes_xtime(d^a);
    }
}

void aes_add_round_key(__private uchar s[16], __private const uchar k[16])
{ for (int i=0;i<16;i++) s[i]^=k[i]; }

void aes_round(__private uchar s[16], __private const uchar k[16])
{ aes_sub_bytes(s); aes_shift_rows(s); aes_mix_columns(s); aes_add_round_key(s,k); }

void aes_final_round(__private uchar s[16], __private const uchar k[16])
{ aes_sub_bytes(s); aes_shift_rows(s); aes_add_round_key(s,k); }

/* ========================================================================== */
/* Steps 2A/2B/2C: scratchpad — STRIDED layout (best on AMD RDNA)              */
/*                                                                             */
/* Each thread's 256KiB scratchpad is contiguous → fits in L2 cache (RDNA1    */
/* has 4MiB L2). Interleaved layout spreads each thread's data across 2GiB    */
/* → no cache locality on AMD. NVIDIA benefits from interleaved (coalescing)  */
/* but AMD RDNA benefits from strided (L2 cache locality).                     */
/* ========================================================================== */

void fill_scratchpad(__private const uchar seed[32], __global uchar *pad)
{
    __private ulong state[8];
    state[0] = ((__private ulong*)seed)[0];
    state[1] = ((__private ulong*)seed)[1];
    state[2] = ((__private ulong*)seed)[2];
    state[3] = ((__private ulong*)seed)[3];
    state[4] = 0; state[5] = 0; state[6] = 0; state[7] = 0;

    for (uint blk=0;blk<BLOCK_COUNT;blk++) {
        __private ulong out[8];
        sha3_512_65_u64(state, (uchar)(blk & 0xFF), out);

        /* Write 32 bytes (4 u64s) to scratchpad — strided layout */
        uint off = blk * BLOCK_SIZE;
        __global ulong *pb = (__global ulong*)(pad + off);
        pb[0] = out[0]; pb[1] = out[1]; pb[2] = out[2]; pb[3] = out[3];

        /* Chain state: first 4 u64s from output, rest zero */
        state[0] = out[0]; state[1] = out[1];
        state[2] = out[2]; state[3] = out[3];
        state[4] = 0; state[5] = 0; state[6] = 0; state[7] = 0;
    }
}

void sequential_passes(__global uchar *pad)
{
    /* Forward pass: XOR each block with previous (wrap-around) */
    __global ulong *prev_pb = (__global ulong*)(pad + (BLOCK_COUNT-1)*BLOCK_SIZE);
    ulong prev0 = prev_pb[0], prev1 = prev_pb[1], prev2 = prev_pb[2], prev3 = prev_pb[3];
    for (uint i=0;i<BLOCK_COUNT;i++) {
        __global ulong *pb = (__global ulong*)(pad + i*BLOCK_SIZE);
        ulong cv0 = pb[0] ^ prev0, cv1 = pb[1] ^ prev1, cv2 = pb[2] ^ prev2, cv3 = pb[3] ^ prev3;
        pb[0] = cv0; pb[1] = cv1; pb[2] = cv2; pb[3] = cv3;
        prev0 = cv0; prev1 = cv1; prev2 = cv2; prev3 = cv3;
    }
    /* Backward pass: XOR each block with next (wrap-around) */
    __global ulong *next_pb = (__global ulong*)(pad + 0);
    ulong next0 = next_pb[0], next1 = next_pb[1], next2 = next_pb[2], next3 = next_pb[3];
    for (uint i=BLOCK_COUNT;i>0;i--) {
        uint idx=i-1;
        __global ulong *pb = (__global ulong*)(pad + idx*BLOCK_SIZE);
        ulong cv0 = pb[0] ^ next0, cv1 = pb[1] ^ next1, cv2 = pb[2] ^ next2, cv3 = pb[3] ^ next3;
        pb[0] = cv0; pb[1] = cv1; pb[2] = cv2; pb[3] = cv3;
        next0 = cv0; next1 = cv1; next2 = cv2; next3 = cv3;
    }
}

void random_read_mix(__private const uchar seed[32], __global const uchar *pad, __private uchar out[32])
{
    __private ulong acc[4];
    acc[0] = ((__private ulong*)seed)[0];
    acc[1] = ((__private ulong*)seed)[1];
    acc[2] = ((__private ulong*)seed)[2];
    acc[3] = ((__private ulong*)seed)[3];
    ulong pos=0;
    for (ulong r=0;r<RANDOM_READS;r++) {
        uint off=(uint)(pos*BLOCK_SIZE);
        __global const ulong *pb = (__global const ulong*)(pad + off);
        acc[0] ^= pb[0]; acc[1] ^= pb[1]; acc[2] ^= pb[2]; acc[3] ^= pb[3];
        pos=(acc[0] ^ pos ^ r) % BLOCK_COUNT;
    }
    ((__private ulong*)out)[0] = acc[0];
    ((__private ulong*)out)[1] = acc[1];
    ((__private ulong*)out)[2] = acc[2];
    ((__private ulong*)out)[3] = acc[3];
}

/* ========================================================================== */
/* Step 3: AES-128 CTR mix — identical to v1                                  */
/* ========================================================================== */

void aes128_mix(__private const uchar seed[32], ulong nonce, __private uchar out[32])
{
    // 8-byte aligned buffers to prevent GPU_CPU_MISMATCH (Metal fix)
    __private ulong key_aligned[2];  // 2 * 8 = 16 bytes, 8-byte aligned
    __private uchar *key = (__private uchar*)key_aligned;
    for (int i=0;i<16;i++) key[i]=seed[i];
    
    __private ulong counter_aligned[2];  // 2 * 8 = 16 bytes, 8-byte aligned
    __private uchar *counter = (__private uchar*)counter_aligned;
    for (int i=0;i<8;i++) counter[i]    =(uchar)(nonce>>(i*8));
    for (int i=0;i<8;i++) counter[8+i]  =seed[16+i];
    
    __private ulong block0_aligned[2];  // 2 * 8 = 16 bytes, 8-byte aligned
    __private uchar *block0 = (__private uchar*)block0_aligned;
    __private ulong block1_aligned[2];  // 2 * 8 = 16 bytes, 8-byte aligned
    __private uchar *block1 = (__private uchar*)block1_aligned;
    
    for (int i=0;i<16;i++) { block0[i]=counter[i]; block1[i]=counter[i]; }
    uint carry=1;
    for (int i=0;i<16;i++) {
        uint s=(uint)block1[i]+carry;
        block1[i]=(uchar)(s&0xFF);
        carry=s>>8;
        if (carry==0) break;
    }
    for (int r=0;r<3;r++) { aes_round(block0,key); aes_round(block1,key); }
    aes_final_round(block0,key);
    aes_final_round(block1,key);
    for (int i=0;i<16;i++) { out[i]=block0[i]^seed[i]; out[16+i]=block1[i]^seed[16+i]; }
}

/* ========================================================================== */
/* Step 4: Thermal loop — the only addition over v1                           */
/*                                                                             */
/* 8 independent ulong chains, 16384 iters.                                   */
/* Integer-only (no float) = deterministic on all GPU drivers.                */
/* Results XORed back into data[0..16] to prevent dead-code elimination.      */
/* Identical logic in deeksha_lite_fire.rs (CPU reference).                   */
/* ========================================================================== */

void thermal_loop(__private uchar data[32] __attribute__((aligned(8))), ulong nonce)
{
    ulong a = nonce ^ 0x9E3779B97F4A7C15UL;
    ulong b = nonce ^ 0xBF58476D1CE4E5B9UL;
    ulong c = nonce ^ 0x94D049BB133111EBUL;
    ulong d = nonce ^ 0x5851F42D4C957F2DUL;
    ulong e = nonce ^ 0xC0FFEE123456789AUL;
    ulong f = nonce ^ 0xDEADBEEFCAFEBABEUL;
    ulong g = nonce ^ 0xBADC0FFEE0DDF00DUL;
    ulong h = nonce ^ 0xFEEDFACECAFEBEEFUL;

    for (int i = 0; i < THERMAL_ITERS; i++) {
        a = ROL64(a,17) + b;  b = ROL64(b,31) ^ a;
        c = ROL64(c,13) + d;  d = ROL64(d,47) ^ c;
        e = ROL64(e,23) + f;  f = ROL64(f,41) ^ e;
        g = ROL64(g,11) + h;  h = ROL64(h,53) ^ g;
        a = a * 0xFF51AFD7ED558CCDUL;  b = b + 0xFF51AFD7ED558CCDUL;
        c = c * 0x94D049BB133111EBUL;  d = d + 0x5851F42D4C957F2DUL;
        e = e * 0xC0FFEE123456789AUL;  f = f + 0xDEADBEEFCAFEBABEUL;
        g = g * 0xBADC0FFEE0DDF00DUL;  h = h + 0xFEEDFACECAFEBEEFUL;
        a ^= (ulong)data[(i    ) & 0x1F];
        b ^= (ulong)data[(i + 8) & 0x1F];
        c ^= (ulong)data[(i +16) & 0x1F];
        d ^= (ulong)data[(i +24) & 0x1F];
        e ^= (ulong)data[(i + 4) & 0x1F];
        f ^= (ulong)data[(i +12) & 0x1F];
        g ^= (ulong)data[(i + 2) & 0x1F];
        h ^= (ulong)data[(i + 6) & 0x1F];
    }
    /* Fold back — prevents compiler from eliminating the loop */
    data[ 0] ^= (uchar)(a);       data[ 1] ^= (uchar)(a>>8);
    data[ 2] ^= (uchar)(b);       data[ 3] ^= (uchar)(b>>8);
    data[ 4] ^= (uchar)(c);       data[ 5] ^= (uchar)(c>>8);
    data[ 6] ^= (uchar)(d);       data[ 7] ^= (uchar)(d>>8);
    data[ 8] ^= (uchar)(e);       data[ 9] ^= (uchar)(e>>8);
    data[10] ^= (uchar)(f);       data[11] ^= (uchar)(f>>8);
    data[12] ^= (uchar)(g);       data[13] ^= (uchar)(g>>8);
    data[14] ^= (uchar)(h);       data[15] ^= (uchar)(h>>8);
    data[16] ^= (uchar)(a>>16);   data[17] ^= (uchar)(b>>16);
    data[18] ^= (uchar)(c>>16);   data[19] ^= (uchar)(d>>16);
    data[20] ^= (uchar)(e>>16);   data[21] ^= (uchar)(f>>16);
    data[22] ^= (uchar)(g>>16);   data[23] ^= (uchar)(h>>16);
    data[24] ^= (uchar)(a>>24);   data[25] ^= (uchar)(b>>24);
}

/* ========================================================================== */
/* Stream-profit byproduct helpers                                             */
/*                                                                             */
/* These perform extra work proportional to stream weights AFTER the base      */
/* PoW hash has been computed.  Results are written back to the scratchpad     */
/* (which is then discarded) so the compiler cannot dead-code eliminate the    */
/* extra work, while the PoW output hash remains unchanged.                    */
/* ========================================================================== */

#define STREAM_WEIGHT_COUNT 6
#define SW_ZION          0
#define SW_KECCAK_BONUS  1
#define SW_SHA3_BONUS    2
#define SW_NCL_AI        3
#define SW_DEEKSHA_LITE  4
#define SW_THERMAL       5

#define STREAM_ITERS_SCALE 16.0f

void stream_byproduct_keccak(__private const uchar in[32], int iters, __global uchar *pad)
{
    if (iters <= 0) return;
    keccak_st_t s;
    for (int i = 0; i < 25; i++) s.u[i] = 0;
    for (int i = 0; i < 32; i++) s.b[i] ^= in[i];
    s.b[32] ^= 0x01;
    s.b[135] ^= 0x80;
    for (int i = 0; i < iters; i++) {
        keccak_f1600(s.u);
    }
    ulong4 h = vload4(0, (__private ulong*)s.b);
    vstore4(h, 0, (__global ulong*)pad);
}

void stream_byproduct_sha3(__private const uchar in[32], int iters, __global uchar *pad)
{
    if (iters <= 0) return;
    uchar tmp[64];
    for (int i = 0; i < 32; i++) tmp[i] = in[i];
    for (int i = 32; i < 64; i++) tmp[i] = 0;
    for (int i = 0; i < iters; i++) {
        sha3_512(tmp, 32, tmp);
    }
    ulong4 h = vload4(0, (__private ulong*)tmp);
    vstore4(h, 0, (__global ulong*)pad);
}

void stream_byproduct_aes(__private const uchar in[32], ulong nonce, int iters, __global uchar *pad)
{
    if (iters <= 0) return;
    uchar tmp[32];
    for (int i = 0; i < 32; i++) tmp[i] = in[i];
    for (int i = 0; i < iters; i++) {
        aes128_mix(tmp, nonce + (ulong)i, tmp);
    }
    ulong4 h = vload4(0, (__private ulong*)tmp);
    vstore4(h, 0, (__global ulong*)pad);
}

/* ========================================================================== */
/* Main kernel                                                                  */
/* ========================================================================== */

__kernel void deeksha_lite_fire_mine(
    __global const ulong *header_keccak_state,  /* same as v1: host precomputed */
    ulong  nonce_base,
    uint   nonce_count,
    __global uchar *output_hashes,
    __global uchar *scratchpad_pool,
    __constant float *stream_weights)
{
    uint tid = get_global_id(0);
    if (tid >= nonce_count) return;

    ulong nonce = nonce_base + (ulong)tid;

    /* Step 1: Keccak256(header || nonce) — same as v1 */
    uchar s1[32];
    keccak256_from_state(header_keccak_state, nonce, s1);

    /* Step 2: Memory-hard scratchpad — STRIDED (per-thread 256KiB contiguous) */
    __global uchar *pad = scratchpad_pool + (ulong)tid * (ulong)SCRATCHPAD_SIZE;
    fill_scratchpad(s1, pad);
    sequential_passes(pad);
    uchar s2[32];
    random_read_mix(s1, pad, s2);

    /* Step 3: AES-128 CTR mix — same as v1 */
    uchar s3[32];
    aes128_mix(s2, nonce, s3);

    /* Step 4: Thermal loop — extra heat, not in v1 */
    thermal_loop(s3, nonce);

    /* Step 5: Keccak256 final — u64-optimized, matches CUDA */
    __private ulong st[25];
    st[0]=((__private ulong*)s3)[0]; st[1]=((__private ulong*)s3)[1];
    st[2]=((__private ulong*)s3)[2]; st[3]=((__private ulong*)s3)[3];
    st[4]=0; st[5]=0; st[6]=0; st[7]=0; st[8]=0; st[9]=0;
    st[10]=0; st[11]=0; st[12]=0; st[13]=0; st[14]=0; st[15]=0;
    st[16]=0; st[17]=0; st[18]=0; st[19]=0; st[20]=0; st[21]=0;
    st[22]=0; st[23]=0; st[24]=0;
    st[4] ^= 0x01UL;          /* 0x01 at byte 32 = st[4] byte 0 */
    st[16] ^= (0x80UL << 56); /* 0x80 at byte 135 = st[16] byte 7 */
    keccak_f1600(st);

    __global uchar *slot = output_hashes + (ulong)tid * 32;
    __global ulong *slot_u64 = (__global ulong*)slot;
    slot_u64[0] = st[0]; slot_u64[1] = st[1]; slot_u64[2] = st[2]; slot_u64[3] = st[3];

    /* hash is st[0..3] — use for stream byproduct */
    __private ulong hash_u64[4];
    hash_u64[0] = st[0]; hash_u64[1] = st[1]; hash_u64[2] = st[2]; hash_u64[3] = st[3];
    __private uchar *hash = (__private uchar*)hash_u64;

    /* Stream-profit byproduct work (does not affect PoW hash).
     * NOTE: stream_byproduct_* write to pad[0..16] which is already consumed
     * by random_read_mix. The compiler cannot DCE because it's a global
     * memory write with a dependency on hash. */
    if (stream_weights) {
        int keccak_iter = (int)(stream_weights[SW_KECCAK_BONUS] * STREAM_ITERS_SCALE);
        stream_byproduct_keccak(hash, keccak_iter, pad);

        int sha3_iter = (int)(stream_weights[SW_SHA3_BONUS] * STREAM_ITERS_SCALE);
        stream_byproduct_sha3(hash, sha3_iter, pad);

        float aes_weight = stream_weights[SW_NCL_AI] + stream_weights[SW_DEEKSHA_LITE] + stream_weights[SW_THERMAL];
        int aes_iter = (int)(aes_weight * STREAM_ITERS_SCALE);
        stream_byproduct_aes(hash, nonce, aes_iter, pad);

        int zion_iter = (int)(stream_weights[SW_ZION] * STREAM_ITERS_SCALE);
        stream_byproduct_keccak(hash, zion_iter, pad);
    }
}
