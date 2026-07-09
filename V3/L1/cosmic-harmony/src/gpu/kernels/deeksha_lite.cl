/*
 * DeekshaLite v1 — OpenCL Kernel (GCN/RDNA compatible) OPTIMIZED
 *
 * Pipeline (matches CPU deeksha_lite.rs exactly):
 *   1. Keccak256(header[0..80] || nonce_le[0..8])  → s1[32]
 *      OPTIMIZATION: Host precomputes Keccak state after absorbing header.
 *      Each thread only XORs nonce, applies padding, and runs f1600.
 *   2. Memory-hard scratchpad (256 KiB)
 *        Phase A: SHA3-512 chain fill  (BLOCK_COUNT=8192 × 32B)
 *        Phase B: 2 sequential XOR passes (forward, backward)
 *        Phase C: 64 random reads → acc[32]  (idx derived from 8 bytes)
 *      OPTIMIZATION: Vectorized 32-byte reads/writes via ulong4 vload4/vstore4.
 *   3. AES-128 CTR mix (key=s2[0..16], counter=nonce||s2[16..24])
 *        → block0 + block1(counter+1), 3 full rounds + 1 final
 *        → XOR with s2[0..32]
 *   4. Keccak256(s3)  → final hash[32]
 *
 * GCN-safe: union instead of pointer casts for keccak state.
 * No Blake3 — SHA3-512 is used for scratchpad fill (GPU-friendly).
 */

#pragma OPENCL EXTENSION cl_khr_int64_base_atomics : enable

/* ========================================================================== */
/* Constants                                                                   */
/* ========================================================================== */

#define SCRATCHPAD_SIZE  262144   /* 256 KiB = 8192 * 32 */
#define BLOCK_SIZE       32
#define BLOCK_COUNT      8192
#define PASSES           2
#define RANDOM_READS     64

/* ========================================================================== */
/* Keccak — canonical implementation from cosmic_harmony_deeksha.cl           */
/* Uses rotate(long,long) per AMD GCN/RDNA workaround recommendation.         */
/* ========================================================================== */

/* AMD Vega/GCN/RDNA: use rotate(long,long) — correct on all AMD targets */
#define ROL64(x, n) rotate((long)((ulong)(x)), (long)((ulong)(n)))

/* Chi macro: one 5-element row, no temp array */
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

/* keccak_f1600: canonical Rho+Pi via 23-element swap chain (no arrays) */
void keccak_f1600(__private ulong *st)
{
    ulong bc0, bc1, bc2, bc3, bc4, t;

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

        /* Rho+Pi — 23-element swap chain (same order as canonical reference) */
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
        st[0] ^= KC_RC[rnd];
    }
}

/* keccak_state union — GCN address space safe (no pointer casts) */
typedef union { ulong u[25]; uchar b[200]; } keccak_st_t;

/* Keccak256 (Ethereum variant, padding 0x01) — precomputed state variant */
void keccak256_from_state(
    __global const ulong *pre_state,
    ulong nonce,
    __private uchar out[32])
{
    keccak_st_t s;
    for (int i = 0; i < 25; i++) s.u[i] = pre_state[i];
    /* XOR nonce into bytes 80..87 */
    for (int i = 0; i < 8; i++)
        s.b[80 + i] ^= (uchar)(nonce >> (i * 8));
    /* Apply padding */
    s.b[88]  ^= 0x01;
    s.b[135] ^= 0x80;
    keccak_f1600(s.u);
    for (int i = 0; i < 32; i++) out[i] = s.b[i];
}

/* SHA3-512 (NIST, padding 0x06, rate=72) */
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

/* ========================================================================== */
/* AES-128 helpers                                                             */
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

void aes_sub_bytes(__private uchar s[16])
{
    for (int i = 0; i < 16; i++) s[i] = AES_SBOX[s[i]];
}

void aes_shift_rows(__private uchar s[16])
{
    uchar t;
    t = s[1];  s[1]  = s[5];  s[5]  = s[9];  s[9]  = s[13]; s[13] = t;
    t = s[2];  s[2]  = s[10]; s[10] = t;
    t = s[6];  s[6]  = s[14]; s[14] = t;
    t = s[15]; s[15] = s[11]; s[11] = s[7];   s[7]  = s[3];  s[3]  = t;
}

uchar aes_xtime(uchar a)
{
    return (uchar)((a << 1) ^ (((a >> 7) & 1) * 0x1b));
}

void aes_mix_columns(__private uchar s[16])
{
    for (int i = 0; i < 4; i++) {
        uchar a = s[i*4], b = s[i*4+1], c = s[i*4+2], d = s[i*4+3];
        uchar e = a ^ b ^ c ^ d;
        s[i*4]   ^= e ^ aes_xtime(a ^ b);
        s[i*4+1] ^= e ^ aes_xtime(b ^ c);
        s[i*4+2] ^= e ^ aes_xtime(c ^ d);
        s[i*4+3] ^= e ^ aes_xtime(d ^ a);
    }
}

void aes_add_round_key(__private uchar s[16], __private const uchar k[16])
{
    for (int i = 0; i < 16; i++) s[i] ^= k[i];
}

void aes_round(__private uchar s[16], __private const uchar k[16])
{
    aes_sub_bytes(s);
    aes_shift_rows(s);
    aes_mix_columns(s);
    aes_add_round_key(s, k);
}

void aes_final_round(__private uchar s[16], __private const uchar k[16])
{
    aes_sub_bytes(s);
    aes_shift_rows(s);
    aes_add_round_key(s, k);
}

/* ========================================================================== */
/* Step 2A: Fill scratchpad with SHA3-512 chain                               */
/*                                                                             */
/* Matches CPU deeksha_lite.rs step2_memory_hard Phase 1:                     */
/*   state[0..32] = seed, state[32..64] = 0                                   */
/*   for blk in 0..4096:                                                       */
/*     input[0..64] = state                                                    */
/*     input[64..68] = blk.to_le_bytes()  (only [64] used — hash 65 bytes)   */
/*     out = sha3_512(&input[..65])                                            */
/*     pad[blk*32..blk*32+32] = out[0..32]                                    */
/*     state[0..32] = out[0..32]                                               */
/* ========================================================================== */

void fill_scratchpad(
    __private const uchar seed[32],
    __global uchar *pad)
{
    uchar state[64];
    for (int i = 0; i < 32; i++) state[i] = seed[i];
    for (int i = 32; i < 64; i++) state[i] = 0;

    for (uint blk = 0; blk < BLOCK_COUNT; blk++) {
        uchar inp[65];
        for (int i = 0; i < 64; i++) inp[i] = state[i];
        /* Only low byte of blk index — matches &input[..65] in CPU */
        inp[64] = (uchar)(blk & 0xFF);

        uchar out64[64];
        sha3_512(inp, 65, out64);

        uint off = blk * BLOCK_SIZE;
        /* Vectorized 32-byte write */
        ulong4 v = vload4(0, (__private ulong*)out64);
        vstore4(v, 0, (__global ulong*)(pad + off));
        vstore4(v, 0, (__private ulong*)state);
    }
}

/* ========================================================================== */
/* Step 2B: Sequential passes                                                  */
/*                                                                             */
/* Matches CPU Phase 2 exactly:                                                */
/*   pass 0 (forward):  for i in 0..4096: pad[i] ^= pad[i==0 ? 4095 : i-1]  */
/*   pass 1 (backward): for i in 4095..=0: pad[i] ^= pad[i+1==4096 ? 0: i+1]*/
/* ========================================================================== */

void sequential_passes(__global uchar *pad)
{
    /* Pass 0 — forward */
    for (uint i = 0; i < BLOCK_COUNT; i++) {
        uint prev = (i == 0) ? (BLOCK_COUNT - 1) : (i - 1);
        uint cur  = i * BLOCK_SIZE;
        uint prv  = prev * BLOCK_SIZE;
        ulong4 cur_v = vload4(0, (__global ulong*)(pad + cur));
        ulong4 prv_v = vload4(0, (__global ulong*)(pad + prv));
        cur_v ^= prv_v;
        vstore4(cur_v, 0, (__global ulong*)(pad + cur));
    }
    /* Pass 1 — backward */
    for (uint i = BLOCK_COUNT; i > 0; i--) {
        uint idx  = i - 1;
        uint next = (idx + 1 == BLOCK_COUNT) ? 0 : (idx + 1);
        uint cur  = idx  * BLOCK_SIZE;
        uint nxt  = next * BLOCK_SIZE;
        ulong4 cur_v = vload4(0, (__global ulong*)(pad + cur));
        ulong4 nxt_v = vload4(0, (__global ulong*)(pad + nxt));
        cur_v ^= nxt_v;
        vstore4(cur_v, 0, (__global ulong*)(pad + cur));
    }
}

/* ========================================================================== */
/* Step 2C: Random read mix                                                    */
/*                                                                             */
/* FIX: idx_val reads 8 bytes (u64), matching CPU:                            */
/*   let mut idx_val: u64 = 0;                                                 */
/*   for i in 0..8 { idx_val |= (acc[i] as u64) << (i * 8); }                */
/*   pos = ((idx_val ^ pos as u64 ^ r as u64) as usize) % BLOCK_COUNT;        */
/* ========================================================================== */

void random_read_mix(
    __private const uchar seed[32],
    __global const uchar *pad,
    __private uchar out[32])
{
    uchar acc[32];
    for (int i = 0; i < 32; i++) acc[i] = seed[i];

    ulong pos = 0;
    for (ulong r = 0; r < RANDOM_READS; r++) {
        uint off = (uint)(pos * BLOCK_SIZE);
        /* Vectorized 32-byte XOR */
        ulong4 acc_v = vload4(0, (__private ulong*)acc);
        ulong4 pad_v = vload4(0, (__global const ulong*)(pad + off));
        acc_v ^= pad_v;
        vstore4(acc_v, 0, (__private ulong*)acc);

        /* Read 8 bytes for idx — matches u64 in CPU */
        ulong idx_val = 0;
        for (int i = 0; i < 8; i++)
            idx_val |= ((ulong)acc[i]) << (i * 8);

        pos = (idx_val ^ pos ^ r) % BLOCK_COUNT;
    }

    for (int i = 0; i < 32; i++) out[i] = acc[i];
}

/* ========================================================================== */
/* Step 3: AES-128 CTR mix                                                     */
/*                                                                             */
/* FIX: counter+1 uses proper carry propagation matching CPU:                  */
/*   let mut carry: u16 = 1;                                                   */
/*   for i in 0..16 { sum = block1[i] + carry; block1[i]=sum&0xFF; carry=sum>>8; if carry==0 break } */
/* ========================================================================== */

void aes128_mix(
    __private const uchar seed[32],
    ulong nonce,
    __private uchar out[32])
{
    uchar key[16];
    for (int i = 0; i < 16; i++) key[i] = seed[i];

    uchar counter[16];
    for (int i = 0; i < 8; i++) counter[i]     = (uchar)(nonce >> (i * 8));
    for (int i = 0; i < 8; i++) counter[8 + i] = seed[16 + i];

    uchar block0[16], block1[16];
    for (int i = 0; i < 16; i++) { block0[i] = counter[i]; block1[i] = counter[i]; }

    /* Proper carry propagation for counter+1 */
    uint carry = 1;
    for (int i = 0; i < 16; i++) {
        uint s = (uint)block1[i] + carry;
        block1[i] = (uchar)(s & 0xFF);
        carry = s >> 8;
        if (carry == 0) break;
    }

    for (int r = 0; r < 3; r++) {
        aes_round(block0, key);
        aes_round(block1, key);
    }
    aes_final_round(block0, key);
    aes_final_round(block1, key);

    for (int i = 0; i < 16; i++) {
        out[i]      = block0[i] ^ seed[i];
        out[16 + i] = block1[i] ^ seed[16 + i];
    }
}

/* ========================================================================== */
/* Main kernel                                                                  */
/* ========================================================================== */

__kernel void deeksha_lite_mine(
    __global const ulong *header_keccak_state,
    ulong  nonce_base,
    uint   nonce_count,
    __global uchar *output_hashes,
    __global uchar *scratchpad_pool)
{
    uint tid = get_global_id(0);
    if (tid >= nonce_count) return;

    __global uchar *pad = scratchpad_pool + (ulong)tid * SCRATCHPAD_SIZE;
    ulong nonce = nonce_base + (ulong)tid;

    /* Step 1: Keccak256(header || nonce) using host-precomputed state */
    uchar s1[32];
    keccak256_from_state(header_keccak_state, nonce, s1);

    /* Step 2: Memory-hard scratchpad */
    fill_scratchpad(s1, pad);
    sequential_passes(pad);
    uchar s2[32];
    random_read_mix(s1, pad, s2);

    /* Step 3: AES-128 CTR mix */
    uchar s3[32];
    aes128_mix(s2, nonce, s3);

    /* Step 4: Keccak256 final */
    uchar hash[32];
    /* Reuse sha3_512 path: 32B fits in single rate block, padding at byte 32 */
    keccak_st_t s;
    for (int i = 0; i < 25; i++) s.u[i] = 0;
    for (int i = 0; i < 32; i++) s.b[i] ^= s3[i];
    s.b[32] ^= 0x01;
    s.b[135] ^= 0x80;
    keccak_f1600(s.u);
    for (int i = 0; i < 32; i++) hash[i] = s.b[i];

    __global uchar *slot = output_hashes + (ulong)tid * 32;
    /* Vectorized 32-byte write */
    ulong4 h = vload4(0, (__private ulong*)hash);
    vstore4(h, 0, (__global ulong*)slot);
}
