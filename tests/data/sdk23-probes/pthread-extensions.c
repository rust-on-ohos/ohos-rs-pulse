#define _GNU_SOURCE
#include <pthread.h>

int (*probe)(pthread_t, void **) = pthread_tryjoin_np;
