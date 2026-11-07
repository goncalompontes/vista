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
streaming session. Before implementing UI controls, consult the [Sill reference and source-to-target map](../docs/sill-reference.md) for native SwiftUI design, gesture, layout and pairing patterns; preserve the source license and attribution where code is reused. Refer to `../docs/roadmap.md` for overall progress.
