#define __packed __attribute__((__packed__))
#include <linux/if_pppox.h>

_Static_assert(_Alignof(struct sockaddr_pppox) == 1, "packed layout");
