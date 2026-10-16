#include <linux/sysinfo.h>

_Static_assert(_Generic(((struct sysinfo *)0)->uptime,
                       long: 1, default: 0), "Linux UAPI uptime type");
