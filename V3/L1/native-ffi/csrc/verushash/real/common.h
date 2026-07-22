// Copyright (c) 2014 The Bitcoin Core developers
// Copyright (c) 2016-2023 The Zcash developers
// Distributed under the MIT software license, see the accompanying
// file COPYING or https://www.opensource.org/licenses/mit-license.php .
//
// Minimal standalone version for ZION VerusHash — stripped libsodium,
// bitcoin-config, and Zcash-specific assertions. Only retains endian
// helpers needed by verus_hash.cpp / verus_clhash.cpp.

#ifndef BITCOIN_CRYPTO_COMMON_H
#define BITCOIN_CRYPTO_COMMON_H

#include <stdint.h>
#include <assert.h>
#include <string.h>

#if defined(_WIN32)
#include <winsock2.h>
// Windows winsock2.h does NOT define le16toh/le32toh/le64toh or htole* macros.
// Add them explicitly (Windows is little-endian on x86_64).
#ifndef le16toh
#define le16toh(x) (x)
#endif
#ifndef le32toh
#define le32toh(x) (x)
#endif
#ifndef le64toh
#define le64toh(x) (x)
#endif
#ifndef htole16
#define htole16(x) (x)
#endif
#ifndef htole32
#define htole32(x) (x)
#endif
#ifndef htole64
#define htole64(x) (x)
#endif
#ifndef htobe16
#define htobe16(x) __builtin_bswap16(x)
#endif
#ifndef htobe32
#define htobe32(x) __builtin_bswap32(x)
#endif
#ifndef htobe64
#define htobe64(x) __builtin_bswap64(x)
#endif
#ifndef be16toh
#define be16toh(x) __builtin_bswap16(x)
#endif
#ifndef be32toh
#define be32toh(x) __builtin_bswap32(x)
#endif
#ifndef be64toh
#define be64toh(x) __builtin_bswap64(x)
#endif
#elif defined(__APPLE__)
#include <machine/endian.h>
#include <libkern/OSByteOrder.h>
// macOS doesn't have the Linux endian.h convenience macros — map them
// to OSSwapHostTo{Little,Big}{16,32,64} / OSSwap{Little,Big}ToHost{16,32,64}
#define htole16(x) OSSwapHostToLittleInt16(x)
#define htole32(x) OSSwapHostToLittleInt32(x)
#define htole64(x) OSSwapHostToLittleInt64(x)
#define le16toh(x) OSSwapLittleToHostInt16(x)
#define le32toh(x) OSSwapLittleToHostInt32(x)
#define le64toh(x) OSSwapLittleToHostInt64(x)
#define htobe16(x) OSSwapHostToBigInt16(x)
#define htobe32(x) OSSwapHostToBigInt32(x)
#define htobe64(x) OSSwapHostToBigInt64(x)
#define be16toh(x) OSSwapBigToHostInt16(x)
#define be32toh(x) OSSwapBigToHostInt32(x)
#define be64toh(x) OSSwapBigToHostInt64(x)
#else
#include <arpa/inet.h>
#include <endian.h>
#endif

// sodium.h stub — not needed for VerusHash
static inline int sodium_init(void) { return 0; }

uint16_t static inline ReadLE16(const unsigned char* ptr)
{
    uint16_t x;
    memcpy((char*)&x, ptr, 2);
    return le16toh(x);
}

uint32_t static inline ReadLE32(const unsigned char* ptr)
{
    uint32_t x;
    memcpy((char*)&x, ptr, 4);
    return le32toh(x);
}

uint64_t static inline ReadLE64(const unsigned char* ptr)
{
    uint64_t x;
    memcpy((char*)&x, ptr, 8);
    return le64toh(x);
}

void static inline WriteLE16(unsigned char* ptr, uint16_t x)
{
    uint16_t v = htole16(x);
    memcpy(ptr, (char*)&v, 2);
}

void static inline WriteLE32(unsigned char* ptr, uint32_t x)
{
    uint32_t v = htole32(x);
    memcpy(ptr, (char*)&v, 4);
}

void static inline WriteLE64(unsigned char* ptr, uint64_t x)
{
    uint64_t v = htole64(x);
    memcpy(ptr, (char*)&v, 8);
}

uint32_t static inline ReadBE32(const unsigned char* ptr)
{
    uint32_t x;
    memcpy((char*)&x, ptr, 4);
    return be32toh(x);
}

uint64_t static inline ReadBE64(const unsigned char* ptr)
{
    uint64_t x;
    memcpy((char*)&x, ptr, 8);
    return be64toh(x);
}

void static inline WriteBE32(unsigned char* ptr, uint32_t x)
{
    uint32_t v = htobe32(x);
    memcpy(ptr, (char*)&v, 4);
}

void static inline WriteBE64(unsigned char* ptr, uint64_t x)
{
    uint64_t v = htobe64(x);
    memcpy(ptr, (char*)&v, 8);
}

int inline init_and_check_sodium()
{
    return 0;
}

uint64_t static inline CountBits(uint64_t x)
{
#if defined(__GNUC__) || defined(__clang__)
    return x ? 8 * sizeof(uint64_t) - __builtin_clzll(x) : 0;
#else
    int ret = 0;
    while (x) { x >>= 1; ++ret; }
    return ret;
#endif
}

#endif // BITCOIN_CRYPTO_COMMON_H
