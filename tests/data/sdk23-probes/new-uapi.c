#include <linux/can.h>
#include <linux/futex.h>
#include <linux/mount.h>

_Static_assert(sizeof(struct canxl_frame) > 0, "CAN XL available");
_Static_assert(sizeof(struct futex_waitv) > 0, "futex_waitv available");
_Static_assert(sizeof(struct mount_attr) > 0, "mount attributes available");
