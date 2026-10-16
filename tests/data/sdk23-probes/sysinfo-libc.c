#include <stddef.h>
#include <sys/sysinfo.h>

_Static_assert(sizeof(((struct sysinfo *)0)->__reserved) == 256, "OH libc padding");
_Static_assert(_Generic(((struct sysinfo *)0)->uptime,
                       unsigned long: 1, default: 0), "OH libc uptime type");
