# ARM64 / API 26 full-target blocker inventory

Date: 2026-10-16. SDK: 26.0.0.38 / API 26. Guest: OpenHarmony-7.0.0.39,
aarch64, Linux 6.6.101. Upstream libc: 0.2.190 at
7b0ab5528dc7f361f3a0b4c06c2cad9971617f75.

Scope: all C error diagnostics from the full-target attempt with only the
kernel-version detector patch. This is not a passing full-target result.
Later Rust compilation, linking and runtime may reveal additional blockers.
No SDK headers, libc bindings, test allowlists or guest files were modified
during this investigation. The guest libc was copied read-only for inspection.

## Inventory

The build in `results/upstream-arm64-api26-kernel-probe-001` emitted 361 C error
diagnostics. They group into 138 category/subject pairs, not 138 independent bugs.
Every diagnostic is retained with its original file, line, column and generated
check context in the local diagnostic JSON.

| Category | Diagnostics | Subjects | Finding |
|---|---:|---:|---|
| Header conflicts | 6 | 2 | Undefined __packed; competing sysinfo definitions |
| Missing constants | 121 | 100 | No exact identifier in scanned SDK headers; isolated header probes fail |
| Missing structs | 200 | 8 | SDK lacks complete types; sizeof, alignment, field and roundtrip errors cascade |
| Missing function declarations | 23 | 23 | Absent from public SDK headers and SDK libc exports |
| Missing struct fields | 9 | 3 | can_frame.len8_dlc, ptp_clock_caps.max_phase_adj, sysinfo.__reserved |
| Field type differences | 2 | 2 | ptp_clock_caps.rsv array length; sysinfo.uptime signedness |

All 138 subjects received isolated C probes. 136 reproduced the corresponding
error; two sysinfo checks passed with only the libc header, which identifies
header composition as their trigger. There are no unclassified diagnostics in
this snapshot. The separate unmodified build's kernel-version probe failure is
already documented in [findings](libc-upstream-findings.md).

## 1. Header conflicts

`linux/if_pppox.h` uses `} __packed;` without a definition of __packed in the
minimal inclusion environment. Clang treats it as a variable name, producing
five conflicting declarations. An isolated control defining it as the packed
attribute compiles and confirms alignment 1 for sockaddr_pppox. This is diagnostic
evidence, not a change to the SDK; erasing the attribute would risk changing layout.

`linux/kernel.h` brings in the kernel UAPI sysinfo definition. Including
`sys/sysinfo.h` also introduces OH's libc definition. The latter tests
`_LINUX_KERNEL_H`, while the SDK kernel header uses a different include guard.
The definitions differ: libc has unsigned uptime and a 256-byte __reserved field;
the UAPI definition has signed uptime and different padding. The Rust musl
binding matches those two properties of the libc definition. Isolated libc-header
probes pass. The full translation unit therefore reports both the original
redefinition and secondary field/type errors. This is not evidence by itself
that Rust's libc sysinfo binding is wrong.

## 2. Kernel UAPI availability and struct evolution

The SDK's Linux version marker is 5.10.0. The OH 7.0 musl configuration explicitly
sets `musl_uapi_dir` to `kernel/linux/patches/linux-5.10/prebuilts/usr/include`.
The SDK build copies that UAPI tree separately from its NDK libc headers.
The local if_pppox.h exactly matches the inspected file in that prebuilt tree.
Thus a Linux 6.6 guest does not imply Linux 6.6 development headers in API 26.

Sources: [musl UAPI configuration](https://github.com/openharmony/third_party_musl/blob/3be655a9e7fa78c7698d1575de7c1388455f7189/musl_config.gni),
[SDK header copy targets](https://github.com/openharmony/interface_sdk_c/blob/731b1e08a25e82500df591f2331d560a970b4bbf/third_party/musl/ndk_script/BUILD.gn),
[prebuilt if_pppox.h](https://github.com/openharmony/kernel_linux_patches/blob/4a74180ee538b08acc8f1bc7f13649ceb82ed51f/linux-5.10/prebuilts/usr/include/linux/if_pppox.h).

The 100 missing constants cover CAN/CAN XL/J1939, futex, packet sockets, mount,
TLS, memory advice, TUN, NUMA policy, seccomp, close_range, openat2, netfilter,
netlink/routing, timestamping, memfd, AF_ALG and XDP. The complete per-identifier
inventory and standalone include choices are in `diagnostics.json` and `probes/`.
No claim is made that all were introduced in a particular kernel release or
that the running guest kernel lacks them. This is SDK header availability evidence.

The eight missing types are:

```text
canxl_frame
futex_waitv
mount_attr
tls12_crypto_info_aria_gcm_128
tls12_crypto_info_aria_gcm_256
tls12_crypto_info_chacha20_poly1305
tls12_crypto_info_sm4_ccm
tls12_crypto_info_sm4_gcm
```

The two additional structure-version differences are:

- can_frame has no len8_dlc field in this SDK. Its total C and Rust sizes both
  remain 16 bytes; missing a named field does not alone prove a size mismatch.
- ptp_clock_caps has rsv[12] in the SDK, while Rust has max_phase_adj followed by
  rsv[11]. Both total sizes are 80 bytes. Rust places max_phase_adj at byte 32 and
  rsv at byte 36. This is a field/version difference, not a demonstrated total-size
  ABI break. Behavior of the new field on the guest has not been tested.

Compile-time C and Rust layout controls corroborate those sizes. The full upstream
test checks named fields and their types, so equal total sizes do not make it pass.

## 3. Public SDK surface versus guest libc exports

The 23 undeclared functions have no exact identifier in the SDK header scan and
do not appear in the SDK libc's dynamic export table. Individual expected-header
probes fail even with _GNU_SOURCE. Of these, the following 21 are exported by the
observed guest libc:

```text
addmntent
adjtime
endspent
endutxent
euidaccess
explicit_bzero
gethostid
getspent
getspnam
getutxent
getutxid
getutxline
posix_spawn_file_actions_addchdir_np
posix_spawn_file_actions_addfchdir_np
pthread_getaffinity_np
pthread_setaffinity_np
pututxline
setspent
setutxent
utmpxname
vhangup
```

`pthread_tryjoin_np` and `pthread_timedjoin_np` are absent from both inspected libc
export tables. OH's inspected libc.map.txt comments out both symbols, consistent
with the observation. This does not establish that their implementation source
is absent or that no alternative API exists elsewhere.

The SDK uses a separately maintained `ndk_musl_include` tree and generates its
libc linking library through `ohos_ndk_library` with libc.ndk.json. All 23 names
are absent from that inspected NDK symbol list. The local pthread.h and
sys/sysinfo.h byte hashes match that NDK tree. The system musl header, by contrast,
declares the pthread join extensions. This explains why inspecting system-source
headers alone does not establish public SDK support.

Sources: [NDK pthread.h](https://github.com/openharmony/interface_sdk_c/blob/731b1e08a25e82500df591f2331d560a970b4bbf/third_party/musl/ndk_musl_include/pthread.h),
[NDK libc build target](https://github.com/openharmony/interface_sdk_c/blob/731b1e08a25e82500df591f2331d560a970b4bbf/third_party/musl/ndk_script/adapter/BUILD.gn),
[NDK symbol list](https://github.com/openharmony/interface_sdk_c/blob/731b1e08a25e82500df591f2331d560a970b4bbf/third_party/musl/ndk_script/adapter/libc.ndk.json),
[system export map](https://github.com/openharmony/third_party_musl/blob/3be655a9e7fa78c7698d1575de7c1388455f7189/libc.map.txt).

Two synthetic C symbol-resolution probes fail linking with the SDK for
explicit_bzero and pthread_tryjoin_np. Separately, two minimal Rust programs using
the actual source-tree libc crate also compile far enough to fail at linking
with `undefined symbol` for those names. A Rust layout-only control links
successfully. Therefore adding missing declarations alone would not resolve
these representative public-SDK linking failures.

Export presence is not proof of signature compatibility, functional behavior or
public API support. In particular, some utmp/shadow-related functions may be
stubs. No excluded function was called during this investigation.

## Reproduce the diagnostic inventory

The Rust example is diagnostic tooling only. It reads a preserved failed build,
scans the selected installed SDK, generates compile-only probes, and inventories
the two libc dynamic symbol tables. It does not patch source or expand coverage.

```sh
mkdir -p results/sdk26-diagnostic-input
hdc -t 127.0.0.1:5555 shell \
  'uname -a; param get const.ohos.fullname; param get const.ohos.apiversion; ls -l /system/lib64/libc.so' \
  > results/sdk26-diagnostic-input/guest-identity.log
hdc -t 127.0.0.1:5555 file recv /system/lib64/libc.so \
  "$PWD/results/sdk26-diagnostic-input/guest-libc.so"

cargo run --locked --example libc_sdk_diagnose -- \
  --build results/upstream-arm64-api26-kernel-probe-001 \
  --guest-libc results/sdk26-diagnostic-input/guest-libc.so \
  --guest-identity results/sdk26-diagnostic-input/guest-identity.log \
  --output "results/sdk26-diagnostics-$(date -u +%Y%m%dT%H%M%SZ)-$$"
```

Local evidence from this investigation:

- `results/upstream-arm64-api26-diagnostics-review-001`: reviewed full inventory, 138 probes,
  three successful controls, two C symbol-link failures, copied guest libc,
  captured guest identity, symbol tables and input/source hashes.
- `results/upstream-arm64-api26-diagnostic-input`: inspected upstream source files
  and original guest acquisition record.
- `results/upstream-arm64-api26-rust-symbol-probes`: minimal Rust sources,
  Cargo.lock, two failed link logs and successful layout-control build.

Evidence remains local and ignored by Git; it has not been remotely archived.
Source comparisons explain the inspected SDK surface, not SDK package provenance.
Further decisions about fixes, supported API boundaries and upstream PRs are deferred.
