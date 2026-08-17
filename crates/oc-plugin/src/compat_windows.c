#ifdef _WIN32

#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdarg.h>
#include <time.h>

double __mingw_strtod(const char *nptr, char **endptr) {
    return strtod(nptr, endptr);
}

int __mingw_vsnprintf(char *s, size_t n, const char *format, va_list arg) {
    return vsnprintf(s, n, format, arg);
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
