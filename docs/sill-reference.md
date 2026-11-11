# Sill reference and Vista iOS implementation map

Upstream: https://github.com/Saffsanity/sill (reference snapshot `41e8cb8`, inspected 2026-11-07).
License: [Apache License 2.0](https://github.com/Saffsanity/sill/blob/main/LICENSE).
Local read-only reference checkout: `/home/pontes/projects/sill-upstream-reference` (developer convenience, **not** a Vista build dependency).

## Direction

**Reuse Sill's proven iOS interaction patterns and appropriately licensed Swift source where it is genuinely portable.** Do not rebuild its UI from scratch merely to rename screens. Vista's Linux host, independent-app sessions, authorization and transport remain distinct; Sill's existing wire format and Mac-specific assumptions are not Vista's protocol. Before porting copied source, retain required Apache 2.0 license and attribution, note modifications, check third-party dependencies, and keep the upstream attribution in distribution. Do not imply Sill endorses Vista.

The local Vista SwiftUI shell remains a preview with fictional data, not connected to a Linux server. Establish authenticated pairing and actual catalog semantics before wiring buttons to remote actions. No UI should claim streaming or pairing before those exist.

## Source-to-target guide

| Vista feature | Sill reference | Vista destination / adaptation |
| --- | --- | --- |
| Nearby hosts and saved device tiles | `iOSClient/ConnectionList.swift`, `SavedMacs.swift`, `DiscoveryPolicy.swift` | Paired Linux host list, discovery and connection states; keep tiles/screenshots but replace Mac discovery semantics |
| Pairing card, QR and manual code | `iOSClient/AddMacCard.swift`, `CodeScanner.swift`, `PairingOverlay.swift` | Native opt-in pairing UI; protocol-defined keys/verification must be designed for Vista |
| Stream screen, thumbnail strip, app drawer | `iOSClient/StreamScreen.swift`, `MockCatalog.swift`, `ThumbnailHold.swift` | `client/ios/Vista/Views/StreamPreviewView.swift` and library navigation; split reusable SwiftUI subviews from Sill-specific `StreamClient` |
| Phone portrait compact control surface | `iOSClient/PhonePortraitLayout.swift`, `PortraitStreamScreen.swift` | Dimension-based phone layout and usable virtual trackpad; adapt keys/modifiers to Linux |
| Multi-touch gestures and pointer | `iOSClient/TrackpadView.swift`, `TrackpadGestures.swift`, `InputOverlay.swift`, `StreamZoom.swift` | Preserve deterministic gesture processing; translate outputs into Vista's authorized input events |
| Settings and inline errors | `iOSClient/HostSettingsPanel.swift`, `HostSettingsLedger.swift` | UI structure and optimistic changes; host-backed settings require ACK/state reconciliation |
| Client decoding and pacing | `iOSClient/HEVCDisplayView.swift`, `KeyframeRecovery.swift`, `AudioDecoder.swift` | Study buffering, keyframe recovery, playback; transport/codec must first be negotiated |
| Clipboard and keyboard | `iOSClient/ClipboardChip.swift`, `TextComposition.swift`, `HardwareKeyboardPolicy.swift` | Linux input/IME and explicit clipboard consent; no silent clipboard access |
| First-run guidance | `iOSClient/TourOverlay.swift`, `TourPolicy.swift` | Introduce controls only after their implementation, accessible on iPhone/iPad |

Relevant upstream behavioral specifications: `docs/iphone-portrait-plan.md`, `docs/trackpad-gestures-plan.md`, `docs/remote-access-plan.md`, and `docs/DEVELOPMENT.md`. Review their current contents at implementation time; the reference checkout is not a stable API.

## Implementation order

1. **Validate host capture** on the real Niri desktop. Synthetic GStreamer tests do not establish portal consent or live PipeWire capture.
2. Define a versioned, authenticated Vista host/client protocol and explicit stream lifecycle, with fake-host fixtures for SwiftUI previews and tests.
3. Port/adapt Sill's connection tiles and pairing UI into small independent SwiftUI components, with disabled network actions until secure pairing works.
4. Port/adapt the stream screen, thumbnail strip, compact phone arrangement and trackpad gesture logic, each tested separately from transport. Preserve accessibility, dynamic sizes and landscape/iPad layouts.
5. Integrate decoding, responsive input, settings, audio and clipboard incrementally, using Sill's implementation and tests as references.
6. Verify on an actual iPhone/iPad and in an Xcode simulator; Linux `swift test` tests portable models only.

## Validation boundaries

- Vista's `client/preview/` is a browser mock, **not** executable SwiftUI.
- Linux can run portable `VistaCore` tests when Swift is installed, but cannot compile an iOS application with Apple's SDK.
- Sill's Mac-side capture/virtual display, Bonjour/wired transport and Mac accessibility APIs are not substitutes for Niri IPC, portal/PipeWire capture, nested compositors, or Linux input routing.
- Snapshot `41e8cb8` is a reference, not a vendored submodule. Any future copied files must identify provenance and preserve the appropriate notices.
