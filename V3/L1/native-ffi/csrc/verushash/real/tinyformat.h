/*
 * tinyformat.h — Minimal stub replacing Bitcoin's tinyformat.h
 *
 * Provides just enough of the tinyformat/strprintf interface for
 * verus_clhash.h's LEToHex / HexToLEBuf helper templates.
 *
 * Copyright (c) 2024-2026 Zion Project — MIT License
 */

#ifndef TINYFORMAT_H_
#define TINYFORMAT_H_

#include <cstdio>
#include <cstdarg>
#include <string>
#include <sstream>

/* Minimal strprintf — supports only simple printf-style formatting.
 * The VerusCoin code uses strprintf("%02x", byte) exclusively. */
template <typename... Args>
inline std::string strprintf(const char *fmt, Args... args)
{
    char buf[256];
    std::snprintf(buf, sizeof(buf), fmt, args...);
    return std::string(buf);
}

/* tfm::format — not used in the hashing code, but stub it out */
namespace tfm {
    template <typename... Args>
    inline void format(std::ostream &os, const char *fmt, Args... args)
    {
        char buf[1024];
        std::snprintf(buf, sizeof(buf), fmt, args...);
        os << buf;
    }
} /* namespace tfm */

#endif /* TINYFORMAT_H_ */
