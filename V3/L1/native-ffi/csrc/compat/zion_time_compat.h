/*
 * zion_time_compat.h — minimal Windows shim for clock_gettime / CLOCK_MONOTONIC
 *
 * Include before any file that uses clock_gettime().
 * On non-Windows this header is a no-op.
 */
#pragma once

#ifdef _WIN32
#  ifndef WIN32_LEAN_AND_MEAN
#    define WIN32_LEAN_AND_MEAN
#  endif
#  include <windows.h>
#  include <time.h>

#  ifndef CLOCK_MONOTONIC
#    define CLOCK_MONOTONIC 1
#  endif

#  ifndef CLOCK_REALTIME
#    define CLOCK_REALTIME 0
#  endif

/* Provide clock_gettime if the CRT does not have it (older SDK / MSVC < 19.32) */
#  if !defined(_TIMESPEC_DEFINED)
struct timespec {
    time_t tv_sec;
    long   tv_nsec;
};
#    define _TIMESPEC_DEFINED
#  endif

static __inline int clock_gettime(int clk_id, struct timespec *ts) {
    (void)clk_id;
    LARGE_INTEGER freq, cnt;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&cnt);
    ts->tv_sec  = (time_t)(cnt.QuadPart / freq.QuadPart);
    ts->tv_nsec = (long)(((cnt.QuadPart % freq.QuadPart) * 1000000000LL) / freq.QuadPart);
    return 0;
}
#endif /* _WIN32 */
