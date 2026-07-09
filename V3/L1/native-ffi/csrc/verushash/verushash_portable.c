/*
 * ============================================================================
 *  ZION Native VerusHash v2.2 — Portable Stub (V3 Phase-2)
 *
 *  Full VerusHash v2.2 requires the Haraka-512 permutation with AES-NI
 *  (x86-64) or ARM-crypto extensions (aarch64).  This portable fallback
 *  computes a deterministic Keccak-256 of (header ++ nonce) so the call
 *  path compiles and exercises the FFI boundary on any host (CI, Windows,
 *  QEMU cross-compile).
 *
 *  PRODUCTION NOTE:
 *    Replace the hashing body with the Haraka + CLHash pipeline from
 *    https://github.com/VerusCoin/VerusCoin/tree/master/src/crypto
 *    The function signatures below are the canonical V3 ABI — do not
 *    change them.
 *
 *  Functions exported (matching native_ffi.rs):
 *    void    verushash_init(void)
 *    void    verushash_hash(const uint8_t* header, size_t header_len,
 *                            uint64_t nonce, uint8_t* output)
 *    int32_t verushash_verify(const uint8_t* header, size_t header_len,
 *                              uint64_t nonce, const uint8_t* target)
 *    double  verushash_benchmark(int32_t iterations)
 *    const char* verushash_version(void)
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

/* ---- Minimal Keccak-256 (portable fallback core) ---- */

#define ROTL64V(x,n) (((x)<<(n))|((x)>>(64-(n))))

static const uint64_t VRC[24]={
    0x0000000000000001ULL,0x0000000000008082ULL,0x800000000000808AULL,
    0x8000000080008000ULL,0x000000000000808BULL,0x0000000080000001ULL,
    0x8000000080008081ULL,0x8000000000008009ULL,0x000000000000008AULL,
    0x0000000000000088ULL,0x0000000080008009ULL,0x000000008000000AULL,
    0x000000008000808BULL,0x800000000000008BULL,0x8000000000008089ULL,
    0x8000000000008003ULL,0x8000000000008002ULL,0x8000000000000080ULL,
    0x000000000000800AULL,0x800000008000000AULL,0x8000000080008081ULL,
    0x8000000000008080ULL,0x0000000080000001ULL,0x8000000080008008ULL
};
static const int VRHO[24]={1,3,6,10,15,21,28,36,45,55,2,14,27,41,56,8,25,43,62,18,39,61,20,44};
static const int VPI[24]={10,7,11,17,18,3,5,16,8,21,24,4,15,23,19,13,12,2,20,14,22,9,6,1};

static void vkeccakf(uint64_t s[25]){
    int i,j,r; uint64_t t,bc[5];
    for(r=0;r<24;r++){
        for(i=0;i<5;i++)bc[i]=s[i]^s[i+5]^s[i+10]^s[i+15]^s[i+20];
        for(i=0;i<5;i++){t=bc[(i+4)%5]^ROTL64V(bc[(i+1)%5],1);for(j=0;j<25;j+=5)s[j+i]^=t;}
        t=s[1];
        for(i=0;i<24;i++){j=VPI[i];bc[0]=s[j];s[j]=ROTL64V(t,VRHO[i]);t=bc[0];}
        for(j=0;j<25;j+=5){uint64_t tmp[5];for(i=0;i<5;i++)tmp[i]=s[j+i];
            for(i=0;i<5;i++)s[j+i]=tmp[i]^(~tmp[(i+1)%5]&tmp[(i+2)%5]);}
        s[0]^=VRC[r];
    }
}

static void vkeccak256(const uint8_t*in,size_t inlen,uint8_t*out){
    uint64_t s[25]={0}; uint8_t tmp[144]; size_t r=136,i;
    for(;inlen>=r;inlen-=r,in+=r){for(i=0;i<r/8;i++)s[i]^=((const uint64_t*)in)[i];vkeccakf(s);}
    memcpy(tmp,in,inlen); tmp[inlen++]=0x01;
    memset(tmp+inlen,0,r-inlen); tmp[r-1]|=0x80;
    for(i=0;i<r/8;i++)s[i]^=((uint64_t*)tmp)[i]; vkeccakf(s);
    memcpy(out,s,32);
}

/* ---- Public API ---- */

EXPORT void verushash_init(void) {
    /* no-op for portable stub — real impl inits Haraka lookup tables */
}

EXPORT void verushash_hash(
    const uint8_t* header,
    size_t         header_len,
    uint64_t       nonce,
    uint8_t*       output)
{
    /* Portable fallback: Keccak256(header || nonce_LE8)
     * Real: Haraka512 → CLHash → VerusHash v2.2 pipeline */
    size_t copy = header_len < 72 ? header_len : 72;
    uint8_t buf[80];
    memset(buf, 0, 80);
    memcpy(buf, header, copy);
    for (int i = 0; i < 8; i++) buf[copy + i] = (uint8_t)(nonce >> (8 * i));
    vkeccak256(buf, copy + 8, output);
}

EXPORT int32_t verushash_verify(
    const uint8_t* header,
    size_t         header_len,
    uint64_t       nonce,
    const uint8_t* target)
{
    uint8_t hash[32];
    verushash_hash(header, header_len, nonce, hash);
    for (int i = 31; i >= 0; i--) {
        if (hash[i] < target[i]) return 1;
        if (hash[i] > target[i]) return 0;
    }
    return 1;
}

EXPORT double verushash_benchmark(int32_t iterations) {
    uint8_t header[76] = {0x56}, out[32];
    struct timespec t0, t1;
    timespec_get(&t0, TIME_UTC);
    for (int32_t i = 0; i < iterations; i++) {
        header[0] = (uint8_t)i;
        verushash_hash(header, 76, (uint64_t)i, out);
    }
    timespec_get(&t1, TIME_UTC);
    double secs = (t1.tv_sec - t0.tv_sec) + (t1.tv_nsec - t0.tv_nsec) * 1e-9;
    return secs > 0.0 ? iterations / secs : 0.0;
}

EXPORT const char* verushash_version(void) {
    return "ZION VerusHash v0.1 — portable stub (link VerusCoin Haraka/CLHash for production)";
}
