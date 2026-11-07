# Vista iOS/iPadOS client

A native SwiftUI application shell, with a small Foundation-only Swift package
for model and protocol types. **This is a UI preview, not an active stream.**
The gallery is fictional sample data: no host is paired, no network requests
are made, and no desktop pixels are captured or transmitted.

## Structure

- `core/`: `VistaCore` Swift Package; portable models and tests.
- `ios/project.yml`: XcodeGen source for the iOS app project.
- `ios/Vista/`: SwiftUI UI shell for iPhone and iPad.
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

1. Install vphone-linux in a separate checkout (default: `~/lsm-workspace/vphone-linux`).
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
4. Start the VM with `~/lsm-workspace/vphone-linux/setup/start-vm.sh`.
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

**Current setup status (2026-11-07):** `xtool 1.24.0` was installed
as a SHA-256-verified AppImage, extracted under
`~/.local/opt/xtool-1.24.0/squashfs-root/` and exposed at
`~/.local/bin/xtool`. This avoids FUSE, which was unavailable inside the
DSH sandbox. The vphone-linux source and its pinned submodules were
checked out at `~/lsm-workspace/vphone-linux`; no VM has booted yet.

Swiftly and the pinned **Swift 6.4.1** user-local toolchain have now been
installed; `swift --version` succeeds, but `swift package dump-package`
currently fails because Arch's `libxml2-legacy` (`libxml2.so.2`) is missing.
The remaining prerequisites are that system library, an Apple Xcode
`.xip`/iOS SDK (not bundled or downloadable by Vista), the vphone
system dependencies, working NVIDIA Vulkan device access and the VM's
firmware/restore procedure. The observed DSH shell has no `/dev/dri`,
`/dev/nvidia*` or `/dev/vhost-vsock`, even though the host PCI
device is detected; check from a normal unsandboxed Niri terminal
before diagnosing the driver itself. Required but currently missing
Arch packages included `lld`, `python-setuptools`, `rsync`,
`libslirp`, `vulkan-headers`, `dtc`, `socat`, `sshfs` and
`libxml2-legacy`; `lzfse` is also absent and built from AUR by
the upstream setup script. Review that script before granting sudo,
as it can modify kernel module, udev and sysctl settings.

**Validation boundary:** the xtool package, simulator-like VM and native
SwiftUI compilation must actually be exercised before describing the
environment as working; static project files alone are not a validated setup.
Keep the macOS build/test path as an independent verification source.

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

A local, unpublished `.github/workflows/ios.yml` workflow draft builds the iOS Simulator target on a
macOS runner without device signing and runs `VistaCore` tests. Publishing it is blocked until the GitHub OAuth token has `workflow` scope. The generated
`Vista.xcodeproj` is ignored; regenerate it from `project.yml`.

## Next integration milestones

1. Define an authenticated host API and pairing handshake, not plaintext host
   discovery accepted as trust.
2. Populate the library from a paired Linux host instead of `DemoCatalog`.
3. Add a real low-latency video decoder and audio pipeline once the host
   transport exists.
4. Add touch, pointer, keyboard, clipboard and lifecycle/reconnect behavior,
   each gated by explicit session permission.

The current preview deliberately does not impersonate a connected or
streaming session. Refer to `../docs/roadmap.md` for overall progress.
