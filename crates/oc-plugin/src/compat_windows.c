#ifdef _WIN32

#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdarg.h>
#include <time.h>

void ___chkstk_ms(void) {}

double __mingw_strtod(const char *nptr, char **endptr) {
    return strtod(nptr, endptr);
}

int __mingw_vsnprintf(char *s, size_t n, const char *format, va_list arg) {
    return vsnprintf(s, n, format, arg);
}

int __mingw_snprintf(char *s, size_t n, const char *format, ...) {
    va_list args;
    va_start(args, format);
    int ret = vsnprintf(s, n, format, args);
    va_end(args);
    return ret;
}

int __mingw_sprintf(char *s, const char *format, ...) {
    va_list args;
    va_start(args, format);
    int ret = vsprintf(s, format, args);
    va_end(args);
    return ret;
}

int __mingw_printf(const char *format, ...) {
    va_list args;
    va_start(args, format);
    int ret = vprintf(format, args);
    va_end(args);
    return ret;
}

int __mingw_fprintf(FILE *stream, const char *format, ...) {
    va_list args;
    va_start(args, format);
    int ret = vfprintf(stream, format, args);
    va_end(args);
    return ret;
}

struct timeval {
    long tv_sec;
    long tv_usec;
};

int gettimeofday(struct timeval *tp, void *tzp) {
    (void)tzp;
    if (tp) {
        FILETIME ft;
        GetSystemTimeAsFileTime(&ft);
        unsigned long long t = ((unsigned long long)ft.dwHighDateTime << 32) | ft.dwLowDateTime;
        t -= 116444736000000000ULL;
        tp->tv_sec = (long)(t / 10000000ULL);
        tp->tv_usec = (long)((t % 10000000ULL) / 10ULL);
    }
    return 0;
}

int pthread_mutex_lock(void *m) {
    (void)m;
    return 0;
}

int pthread_mutex_unlock(void *m) {
    (void)m;
    return 0;
}

int pthread_cond_init(void *c, void *attr) {
    (void)c;
    (void)attr;
    return 0;
}

int pthread_cond_destroy(void *c) {
    (void)c;
    return 0;
}

int pthread_cond_signal(void *c) {
    (void)c;
    return 0;
}

int pthread_cond_wait(void *c, void *m) {
    (void)c;
    (void)m;
    return 0;
}

int pthread_cond_timedwait64(void *c, void *m, const void *abstime) {
    (void)c;
    (void)m;
    (void)abstime;
    return 0;
}

int clock_gettime64(int clk_id, void *tp) {
    (void)clk_id;
    (void)tp;
    return 0;
}

#endif

