# Manual SDK 23 ABI experiment

For the local ARM64 / OH 7.0 environment, use the [ARM64 recipe](libc-upstream-arm64.md).

This recipe uses the pinned Oniro v6.1 x86_64 image and the `sdk23-core` upstream
ctest adaptation. The profile checks an explicit file, memory, time and IPv4
socket ABI allowlist. It does not establish full libc compatibility or execute
the selected functions' application behavior. Guest execution is not yet verified.

Run commands from the project root. Keep the existing ARM64 cargo-ohos emulator
on port 5555 separate; this experiment uses port 55555.

## 1. Verify and extract the image

The archive is already downloaded locally. On another machine, download it first:

```sh
mkdir -p results/oniro-v6.1-local
curl -fL --retry 2 --max-time 900 \
  https://github.com/eclipse-oniro4openharmony/device_board_oniro/releases/download/v6.1/oniro_emulator.zip \
  -o results/oniro-v6.1-local/oniro_emulator.zip
```

Verify before extraction. Extract only if the destination does not already exist:

```sh
printf '%s  %s\n' \
  a30e01f1b161e0fdcd91263f30f29cc92bfa49c1776d5ee2c46caefef055b2da \
  results/oniro-v6.1-local/oniro_emulator.zip | shasum -a 256 -c -

if [ ! -d results/oniro-v6.1-local/extracted ]; then
  unzip -q results/oniro-v6.1-local/oniro_emulator.zip \
    -d results/oniro-v6.1-local/extracted
fi
```

## 2. Boot QEMU in terminal A

This headless command follows the pinned emulator Action's launch configuration.
On Apple Silicon, the x86_64 guest uses TCG rather than hardware acceleration.
Leave this terminal running. Stop this instance with Ctrl-A, then X after collecting
evidence. Do not start another instance using the same disk files.

```sh
cd results/oniro-v6.1-local/extracted/images
qemu-system-x86_64 \
  -machine q35 -smp 4 -m 4096M -boot c -nographic \
  -rtc base=utc,clock=host \
  -initrd ramdisk.img -kernel bzImage \
  -drive if=none,file=updater.img,format=raw,id=updater,index=0 \
  -device virtio-blk-pci,drive=updater \
  -drive if=none,file=system.img,format=raw,id=system,index=1 \
  -device virtio-blk-pci,drive=system \
  -drive if=none,file=vendor.img,format=raw,id=vendor,index=2 \
  -device virtio-blk-pci,drive=vendor \
  -drive if=none,file=userdata.img,format=raw,id=userdata,index=3 \
  -device virtio-blk-pci,drive=userdata \
  -append 'ip=dhcp loglevel=4 console=ttyS0,115200 init=init root=/dev/ram0 rw ohos.boot.hardware=x86_general ohos.required_mount.system=/dev/block/vdb@/usr@ext4@ro,barrier=1@wait,required ohos.required_mount.vendor=/dev/block/vdc@/vendor@ext4@ro,barrier=1@wait,required ohos.required_mount.misc=/dev/block/vda@/misc@none@none=@wait,required' \
  -accel tcg,thread=multi -cpu max \
  -netdev user,id=net0,hostfwd=tcp:127.0.0.1:55555-:55555 \
  -device virtio-net-pci,netdev=net0
```

## 3. Connect in terminal B

Start in the project root in this terminal. Use the API 23 hdc executable:

```sh
export OHOS_SDK_NATIVE="$HOME/Library/Caches/cargo-ohos/ohos-sdk/6.1/darwin/23/native"
export PATH="$(dirname "$OHOS_SDK_NATIVE")/toolchains:$PATH"
hdc tconn 127.0.0.1:55555
hdc list targets
hdc -t 127.0.0.1:55555 shell uname -a
```

Retry the connection after boot progresses if the first attempt fails. Confirm
the connected guest is x86_64. All deployment commands explicitly target 55555.

## 4. Build and execute

The existing clean `.upstream-libc` checkout is required at the commit in
`config/upstream-libc.json`. The CLI verifies its identity before applying a
digest-pinned patch to an isolated source copy. The original checkout is unchanged.

```sh
cargo build --locked
pulse_attempt="$(date -u +%Y%m%dT%H%M%SZ)-$$"
pulse_build="results/upstream-manual-$pulse_attempt-build"
pulse_guest="results/upstream-manual-$pulse_attempt-guest"

target/debug/ohos-pulse upstream-build \
  --source .upstream-libc --profile sdk23-core --output "$pulse_build"
```

Only continue if the build exits zero and inspection passes:

```sh
target/debug/ohos-pulse upstream-runtime \
  --build "$pulse_build" --output "$pulse_guest"
cat "$pulse_guest/summary.md"
```

The runtime command checks loader/dependencies, transfers and hashes the binary,
executes it with a timeout in a unique guest directory, captures the actual exit
code and separate output streams, validates the upstream harness completion,
and cleans up. A successful build alone is not a runtime result. Inspect
`upstream.json`, `summary.md`, and `logs/` on either success or failure. The build
currently generates 236 test functions; the harness comparison count is a
different unit and must come from actual execution.

For repeat runs, keep the same build and choose a fresh output directory:

```sh
target/debug/ohos-pulse upstream-runtime --build "$pulse_build" \
  --output "results/upstream-manual-$(date -u +%Y%m%dT%H%M%SZ)-$$-guest"
```

## Other profiles

- `baseline` (default): unmodified upstream source; version detection currently fails.
- `kernel-probe`: only the version detector fix; the full target still fails C compilation.
- `sdk23-core`: bounded ABI allowlist, recorded explicitly as adapted evidence.

The SDK archive provenance gap remains open. The pinned image archive digest is
verified above; do not treat an arbitrary existing emulator as the same image.
