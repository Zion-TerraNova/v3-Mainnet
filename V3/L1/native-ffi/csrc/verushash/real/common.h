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
