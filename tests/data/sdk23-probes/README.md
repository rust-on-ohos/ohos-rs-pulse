# SDK header diagnostic probes

These C files reproduce API 23 x86_64 header failures without Rust or ctest.
They are manual diagnostics, not portable passing tests.

```sh
export OHOS_SDK_NATIVE="$HOME/Library/Caches/cargo-ohos/ohos-sdk/6.1/darwin/23/native"
"$OHOS_SDK_NATIVE/llvm/bin/clang" --target=x86_64-linux-ohos \
  --sysroot="$OHOS_SDK_NATIVE/sysroot" -D__MUSL__ -Werror -ferror-limit=0 \
  -fsyntax-only tests/data/sdk23-probes/packed.c
```

Replace the final filename to run another probe:

| Probe | Observed result | Interpretation |
|---|---|---|
| packed.c | Failed | if_pppox.h uses undefined __packed as though it were an attribute |
| packed-defined.c | Passed | Explicit packed attribute resolves this isolated header failure |
| sysinfo-combined.c | Failed | Linux UAPI and libc headers define competing sysinfo structs |
| sysinfo-libc.c | Passed | Libc header provides unsigned uptime and 256-byte reserved field |
| sysinfo-kernel.c | Passed | Kernel UAPI header provides signed uptime |
| pthread-extensions.c | Failed | pthread_tryjoin_np is not declared, even with _GNU_SOURCE |
| new-uapi.c | Failed | CAN XL, futex_waitv and mount_attr types are incomplete/absent |

The packed definition is a local diagnostic candidate, not an SDK modification.
Do not erase __packed: that could change ABI layout. The sysinfo probes establish
a header-composition problem; they do not establish a Rust sysinfo binding defect.
Missing declarations do not independently prove guest runtime symbol absence.
Results and full stderr are retained locally in results/upstream-sdk23-diagnosis-001.
