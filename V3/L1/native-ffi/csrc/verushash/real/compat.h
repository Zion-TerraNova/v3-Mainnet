/*
 * compat.h  --  Compatibility shim for building VerusCoin crypto code
 *               outside of the full Bitcoin / Zcash source tree.
 *
 * Copyright (c) 2024-2026 Zion Project  --  MIT License
 */

#ifndef VERUSHASH_COMPAT_H
#define VERUSHASH_COMPAT_H

/* Standard C includes */
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <stdlib.h>
#include <assert.h>

/* ---------------------------------------------------------------
 * Architecture detection and intrinsics
 * --------------------------------------------------------------- */
#if defined(__aarch64__) || defined(__arm__) || defined(_M_ARM64)
    #define VERUSHASH_ARM 1
    #include "sse2neon.h"
#elif defined(__x86_64__) || defined(_M_X64) || defined(__i386__) || defined(_M_IX86)
    #define VERUSHASH_X86 1
    #ifdef _MSC_VER
        #include <intrin.h>
    #else
        #include <x86intrin.h>
        #include <cpuid.h>
    #endif
#else
    #define VERUSHASH_PORTABLE_ONLY 1
#endif

/* ---------------------------------------------------------------
 * Boost stubs
 * --------------------------------------------------------------- */
#ifdef __cplusplus

namespace boost {

    template <typename T>
    class thread_specific_ptr {
    public:
        thread_specific_ptr() {}
        ~thread_specific_ptr() {}

        T* get() const { return ptr_; }
        T* operator->() const { return ptr_; }
        T& operator*() const { return *ptr_; }

        void reset(T* p = nullptr) {
            if (ptr_ && ptr_ != p) delete ptr_;
            ptr_ = p;
        }

        T* release() {
            T* tmp = ptr_;
            ptr_ = nullptr;
            return tmp;
        }

    private:
        static thread_local T* ptr_;
    };

    template <typename T>
    thread_local T* thread_specific_ptr<T>::ptr_ = nullptr;

} /* namespace boost */

#endif /* __cplusplus */

/* ---------------------------------------------------------------
 * uint256 minimal stub
 * --------------------------------------------------------------- */
#ifdef __cplusplus

#include <cstring>
#include <algorithm>

class uint256 {
public:
    static constexpr unsigned int WIDTH = 32;
    uint8_t data[WIDTH];

    uint256() { memset(data, 0, WIDTH); }
    uint256(const uint256& other) { memcpy(data, other.data, WIDTH); }

    uint256& operator=(const uint256& other) {
        memcpy(data, other.data, WIDTH);
        return *this;
    }

    const uint8_t* begin() const { return data; }
    const uint8_t* end()   const { return data + WIDTH; }
    uint8_t* begin() { return data; }
    uint8_t* end()   { return data + WIDTH; }

    unsigned int size() const { return WIDTH; }

    void SetNull() { memset(data, 0, WIDTH); }
    bool IsNull() const {
        for (unsigned int i = 0; i < WIDTH; i++)
            if (data[i] != 0) return false;
        return true;
    }

    bool operator==(const uint256& b) const { return memcmp(data, b.data, WIDTH) == 0; }
    bool operator!=(const uint256& b) const { return !(*this == b); }
    bool operator<(const uint256& b)  const { return memcmp(data, b.data, WIDTH) < 0; }
};

/* uint128 stub */
class uint128 {
public:
    uint8_t data[16];
    uint128() { memset(data, 0, 16); }
};

#endif /* __cplusplus */

/* ---------------------------------------------------------------
 * LogPrintf / LogPrint stubs
 * --------------------------------------------------------------- */
#ifdef __cplusplus
#include <cstdio>
#else
#include <stdio.h>
#endif

#ifndef LogPrintf
    #define LogPrintf(...) do { fprintf(stderr, __VA_ARGS__); } while(0)
#endif
#ifndef LogPrint
    #define LogPrint(category, ...) do { (void)(category); } while(0)
#endif

/* Serialize stubs */
#ifndef ADD_SERIALIZE_METHODS
    #define ADD_SERIALIZE_METHODS
#endif
#ifndef READWRITE
    #define READWRITE(x)
#endif

/* Algo selection constants */
#ifdef __cplusplus
#ifndef ASSETCHAINS_VERUSHASH
    #define ASSETCHAINS_VERUSHASH 2
    #define ASSETCHAINS_VERUSHASHV1_1 1
    #define ASSETCHAINS_VERUSHASHV2 2
    #define ASSETCHAINS_VERUSHASHV2_1 3
#endif
#endif

/* Endianness */
#if defined(__BYTE_ORDER__) && (__BYTE_ORDER__ == __ORDER_BIG_ENDIAN__)
    #define VERUSHASH_BIG_ENDIAN 1
#else
    #define VERUSHASH_LITTLE_ENDIAN 1
#endif

#ifndef htole32
    #if defined(VERUSHASH_LITTLE_ENDIAN)
        #define htole32(x) (x)
        #define le32toh(x) (x)
        #define htole64(x) (x)
        #define le64toh(x) (x)
    #else
        #define htole32(x) __builtin_bswap32(x)
        #define le32toh(x) __builtin_bswap32(x)
        #define htole64(x) __builtin_bswap64(x)
        #define le64toh(x) __builtin_bswap64(x)
    #endif
#endif

#endif /* VERUSHASH_COMPAT_H */
