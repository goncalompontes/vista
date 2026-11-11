# Vista iOS/iPadOS client

A native SwiftUI application shell, with a Foundation-only Swift package for
model and protocol types. The iOS app implements host pairing, an authenticated
control session, a bounded live catalog, a separate encrypted H.264 media
connection, and a VideoToolbox-backed stream view with an Annex-B/keyframe gate.
An experimental end-to-end MVP has been exercised in the vphone iOS VM: on
2026-11-10 it paired with the Vista host and displayed live video for the active
output and a virtual desktop. This is evidence of a working video path, not yet
of reliable sustained playback or correct target switching: selecting an
individual window subsequently left a stale frame on screen. Keyboard/pointer
input and host actions are present in the client/host control path but have not
been verified end to end through the iOS UI. Audio and clipboard are not
implemented. The library falls back to clearly labelled sample data only when
no host catalog has been received.

See the [end-to-end test record](../integrations/vista-computer-use/docs/END_TO_END.md#what-was-actually-exercised)
for what was exercised and what remains unverified. The host service is
experimental, single-client and has no TLS; its capture and target-switching
limitations are described in the [root project status](../README.md#status).

## Structure

- `core/`: `VistaCore` Swift Package; portable models, control protocol/pairing crypto and tests (swift-crypto).
- `ios/project.yml`: XcodeGen source for the iOS app project.
- `ios/Vista/`: SwiftUI UI shell for iPhone and iPad; `ios/Vista/Transport/` holds Network.framework and Keychain adapters.
- `preview/`: interactive browser-only iPhone/iPad frame and navigation for rapid UI debugging on Linux (not a simulator).

## Interactive preview on Arch Linux

Run `cd client/preview && ./serve.sh`, then open `http://127.0.0.1:4173/` in Firefox. Use F12 and Ctrl+Shift+M for JavaScript debugging, responsive layouts and touch emulation. The HTML/CSS/JS preview mirrors the SwiftUI shell manually, but **does not execute SwiftUI or iOS code**. See [preview setup](preview/README.md).

## Working from Arch Linux

You can edit Swift source on Arch using VS Code's Swift extension or any LSP
editor with SourceKit-LSP. Install a compatible Swift toolchain for
`swift test` (see [Swift.org](https://www.swift.org/install/)). The
official Swift Linux binaries target specific supported distributions, not
Arch itself; use a compatible installation or supported Linux environment if
needed. Run:

```sh
cd client/core
swift test
```

This tests models, **not** the SwiftUI application. Apple's iOS SDK, Xcode,
SwiftUI build environment and official iOS Simulator require macOS. Linux
cannot independently validate the final iOS application bundle.

## Native iOS development on Arch with vphone-linux (experimental)

Unlike the browser preview, [vphone-linux](https://github.com/YahiaAngelo/vphone-linux)
runs patched genuine iOS firmware under an x86-64 QEMU ARM emulator.
The [xtool](https://github.com/xtool-org/xtool) path builds this repository's
**actual SwiftUI sources**, installs an IPA inside the guest and launches it.
This is development-only: guest graphics, networking, audio, and performance
do not represent a physical iPhone; licensing terms may restrict iOS virtualization
on non-Apple hardware.

Setup prerequisites:

1. Install vphone-linux in a separate checkout (default: `~/projects/vphone-linux`).
   Its own `setup/setup.sh` installs system dependencies and builds QEMU;
   `setup/create-vm.sh` builds a virtual iPhone from Apple firmware. Consult its
   setup instructions and review any root/system changes before proceeding.
   It needs a Vulkan-capable GPU, 16 GiB+ RAM, at least 100 GiB free space,
   and approximately 8 GiB free in `/dev/shm`.
2. Ensure `nvidia-smi` and `vulkaninfo --summary` can see the **real** GPU in
   your desktop session. Sandboxed agent shells can hide `/dev/nvidia*`
   and `/dev/dri`, so GPU diagnostics there may not reflect the host.
3. Install [xtool](https://xtool.sh/documentation/xtooldocs/installation-linux)
   and a compatible Linux Swift toolchain. Obtain Xcode's `.xip` via Apple's
   developer downloads using your Apple account; run `xtool sdk install` on
   the downloaded archive and `xtool sdk status`. The iOS SDK is not bundled
   with Vista or vphone-linux.
4. Start the VM with `~/projects/vphone-linux/setup/start-vm.sh`.
   Confirm its vphoned control interface responds before installing applications.

Both XcodeGen (`client/ios/project.yml`) and xtool
(`client/ios/Package.swift`, `client/ios/xtool.yml`) compile the same
`client/ios/Vista/` SwiftUI files, with the common `client/core` package.
Keep deployment targets, bundle IDs and target source dependencies in sync.

```sh
cd client/ios
./vphone.sh check    # xtool SDK status and VM health
./vphone.sh build    # SwiftUI -> unsigned xtool/Vista.ipa
./vphone.sh run      # build, install, launch as dev.vista.client
./vphone.sh logs     # Vista log entries
./vphone.sh crashes  # Vista crash reports
```

Set `VPHONE_HOME` to point at a different vphone-linux checkout, and
`VPHONE_CID` if the guest uses a vsock CID other than 3. The helper uses
vphone-linux's own IPA installer and vphoned diagnostic tooling.

**Build and simulator setup record (2026-11-09; historical):** Swift 6.4.1, `xtool 1.24.0` and the
**Xcode 27.2 Darwin SDK** are installed (`xtool sdk status` reports the SDK at
`~/.swiftpm/swift-sdks/darwin.artifactbundle`). `xtool dev build --ipa`
produced `client/ios/xtool/Vista.ipa` (about 1.1 MB). The
`.github/workflows/ios.yml` workflow is committed and its two `ios-simulator`
jobs passed on macOS. The vphone iPhone 16 Pro iOS 26.6.2 VM is being created:
the GPU driver was extracted and the firmware prepared/patched; the main
restore/CFW/first-boot stages are still running. Installing and launching the
IPA in the guest is the next validation step.

**Earlier toolchain setup record (2026-11-07):** `xtool 1.24.0` was installed
as a SHA-256-verified AppImage, extracted under
`~/.local/opt/xtool-1.24.0/squashfs-root/` and exposed at
`~/.local/bin/xtool`. This avoids FUSE, which was unavailable inside the
DSH sandbox. The vphone-linux source and its pinned submodules were
checked out at `~/lsm-workspace/vphone-linux` at the time; it has since moved to
`~/projects/vphone-linux`. No VM had booted at that checkpoint.

Swiftly and **Swift 6.4.1** are installed. The Arch `libxml2-legacy`
package (`2.13.9-2`) was downloaded and extracted *without system changes*
into `~/.local/opt/libxml2-legacy-2.13.9-2/`; it is **not** installed by
pacman. For a Swift command in the DSH environment, supply the extracted
runtime library on its own search path:

```sh
export PATH="$HOME/.local/share/swiftly/bin:$PATH"
export LD_LIBRARY_PATH="$HOME/.local/opt/libxml2-legacy-2.13.9-2/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
swift package dump-package --package-path client/ios
swift test --package-path client/core
```

On 2026-11-07, both manifests parsed successfully and all **4 VistaCore
Swift tests passed**. The iOS manifest was corrected to use the local
SwiftPM dependency identity `core` (while importing the `VistaCore` product).
This validates package metadata and portable models, **not** a native iOS
binary. The remaining prerequisites are an Apple Xcode `.xip`/iOS SDK,
the vphone system dependencies, working NVIDIA Vulkan device access and
the VM's firmware/restore procedure. The observed DSH shell has no `/dev/dri`,
`/dev/nvidia*` or `/dev/vhost-vsock`, even though the host PCI
device is detected; check from a normal unsandboxed Niri terminal
before diagnosing the driver itself. Required but currently missing
Arch packages included `lld`, `python-setuptools`, `rsync`,
`libslirp`, `vulkan-headers`, `dtc`, `socat`, `sshfs` and
`libxml2-legacy`; `lzfse` is also absent and built from AUR by
the upstream setup script. Review that script before granting sudo,
as it can modify kernel module, udev and sysctl settings.

**Additional vphone build validation (2026-11-07):** The pinned
`reims-vgpu` Vulkan distribution built successfully, and the QEMU
`qemu-system-aarch64` build completed with exit code 0. The resulting
338 MB executable reports `qemu-vphone-0.1.0_QEMU-10.2.2_10.1.3_hybrid`
and lists the custom `vresearch101` machine. A user-local copy of
`libslirp` initially had incorrect `/usr` paths in its `slirp.pc` metadata;
correcting that staged metadata and supplying the local include/library paths
to Meson resolved the QEMU compilation failure. The patched
`usbmuxd 1.1.1-32k` restore helper also compiled successfully. The Swift
`vphone-cli` firmware tooling also built successfully and its `--help` ran
(exit code 0); the 122 MB executable is at
`vphone-linux/linux-create/.build/out/out/Products/Debug-linux-x86_64/vphone-cli`.
On this Arch host, the firmware build required explicit `-Xcc` LZFSE include
and `-Xlinker` library paths, `-j 6` (not `-j6`), and a user-local
`libxml2.so.2` symlink inside Swift 6.4.1's `usr/lib` so its `ld.lld` can run
when Swift's build subprocesses clear `LD_LIBRARY_PATH`. These are build and
CLI startup checks only, **not** proof that the VM boots or its Vulkan renderer works.
The APFS driver (`linux-apfs-rw` at commit `0318712`, with the repository's
`linux-apfs-rw-ios27.patch`) was built successfully at
`vphone-linux/.build/linux-apfs-rw/apfs.ko` against installed **7.2.14**
headers. `modinfo -F vermagic` reports `7.2.14-arch1-1 SMP preempt mod_unload`.
**Do not load it into the still-running 7.2.11 kernel**: reboot into the
installed 7.2.14 kernel and verify `uname -r` first. The module has not been
loaded or exercised against an APFS filesystem.

A later DSH Full Access verification established /dev/kvm, /dev/vhost-vsock,
/dev/dri/renderD128 and /dev/nvidia0 access. Both Intel and NVIDIA Vulkan ICDs
advertise `VK_EXT_external_memory_host`; actual reims-vgpu rendering and
iOS guest boot remain unverified. The host has about 334 GiB free on /home,
16 GiB free in /dev/shm, and 31 GiB RAM. `rcodesign` (`apple-codesign 0.29.0`)
was installed to `~/.local/bin/rcodesign` and independently verified.
At the time of that 2026-11-07 check, `xtool sdk status` reported **Not installed**;
the 2026-11-09 update above supersedes that: the Xcode 27.2 SDK is installed
and the IPA builds.

The real `setup/create-vm.sh` path remains **unrun**. Its restore stages need
additional host utilities (`socat`, `rsync`, `sshfs`, `bc` were not found),
interactive sudo authentication, and privileged USB/IP helpers. The patched
`usbmuxd-32k.sh` stops, runtime-masks, and replaces the system
`usbmuxd.service` during restore; undo the mask and restart the original
service afterward. Review that service disruption before running the pipeline.
The release bundle ROMs are available, but no completed virtual iPhone,
GPU-driver extraction, installed native IPA, or live SwiftUI interaction has
been verified.

**Validation boundary:** the xtool package, simulator-like VM and native
SwiftUI compilation must actually be exercised before describing the
environment as working; static project files alone are not a validated setup.
Keep the macOS build/test path as an independent verification source.

## Android client (Skip Lite)

`client/android` is an experimental Android port that transpiles the shared
SwiftUI views and `VistaCore` models into Kotlin/Jetpack Compose with Skip Lite.
It builds on Linux (see `client/android/README.md`). The Android application now
uses a native Kotlin Compose entry screen for the real control path: it pairs
with a host, consumes the live window catalog and capability snapshot, and sends
authorized semantic actions over the same encrypted protocol as iOS. The
Skip-generated SwiftUI module remains available for shared presentation work;
live media and native input are still separate increments.

## Physical phone prototype

The current Android control vertical slice is usable on a physical Android
phone once the debug APK is installed and the phone can reach the Linux host
over the LAN. This Android milestone is separate from the experimental iOS
video demonstration recorded in the end-to-end test record. Start the host
listener on a LAN address with an explicit window policy, for example:

```sh
vista-host protocol-serve --confirm --listen 0.0.0.0:7443 \
  --allow-remote --window WINDOW_ID --allow-window-actions --catalog --json
```

Enter the host's address, port and 64-character secret in the Android screen.
The host firewall must allow TCP 7443, and the phone and host must be on a
network that permits device-to-device traffic. The same pairing flow works from
the iOS client, and the vphone test record demonstrates host video in the iOS
VM. That VM result does not validate a physical iPhone or iPad. A physical iOS
device requires a signed device build: macOS with Xcode, an Apple development
team, a provisioned `dev.vista.client` bundle ID, the first-run local-network
permission, and the device trusted by Xcode. The Linux `xtool` IPA is useful for
the vphone experiment but is not a substitute for Apple device signing. The
current iOS video path remains experimental; end-to-end input is unverified,
and audio and clipboard are not implemented.

## Building on a Mac

Install a supported Xcode release and the XcodeGen generator:

```sh
brew install xcodegen
cd client/ios
xcodegen generate
open Vista.xcodeproj
```

Choose the `Vista` scheme and an iOS Simulator in Xcode. For a physical
iPhone/iPad, sign into an Apple Account in Xcode and configure your own
development team and bundle identifier if needed. Free personal teams support
temporary on-device provisioning with restrictions.

The `.github/workflows/ios.yml` workflow builds the iOS Simulator target on a
macOS runner without device signing and runs `VistaCore` tests. Both
`ios-simulator` jobs passed on PR #31. The generated `Vista.xcodeproj` is
ignored; regenerate it from `project.yml`.

## Remaining end-to-end milestones

1. Diagnose stale frames when switching to an individual window, then verify
   sustained frame delivery and reliable stop/switch/reconnect behavior.
2. Verify keyboard and pointer input, plus a safe host action, end to end through
   the iOS UI against a disposable window; keep each action gated by explicit
   session permission and host capability state.
3. Implement and validate audio and clipboard transport and their permission
   paths; neither is currently available.
4. Validate the signed native iOS app on a physical iPhone/iPad independently
   of the vphone VM and Swift package tests.

The current preview deliberately does not impersonate a connected or
streaming session. Before implementing UI controls, consult the [Sill reference and source-to-target map](../docs/sill-reference.md) for native SwiftUI design, gesture, layout and pairing patterns; preserve the source license and attribution where code is reused. Refer to `../docs/roadmap.md` for overall progress.
