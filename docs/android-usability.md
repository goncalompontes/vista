# Android usability milestone

This increment builds on merged PR #103 with a usable native Android client
on the existing v1 host. Implementation and real-host emulator checks cover the
behavior below; see [the evidence and limits](android-verification.md).
Physical-device behavior and complete Sill feature parity are not claimed.

## UI reference

The user confirmed **Sill** (`https://github.com/Saffsanity/sill`) as the UI
reference on 2026-11-11 (inspected snapshot `41e8cb8`). Use its picture-first stream layout, dark control
surfaces, source strip, compact modifier row and explicit keyboard controls,
adapted to Android Material components and Vista's capabilities. The inspected
reference files include `iOSClient/PhonePortraitLayout.swift` and
`iOSClient/StreamScreen.swift` in the read-only reference checkout described by
[the source map](sill-reference.md). This implementation independently expresses
those patterns in Kotlin/Compose; it copies no Sill source. See also the
[interaction inventory](design/sill-inventory.md).

Slint was an earlier ambiguous name in the goal setup. The user's subsequent
confirmation of Sill supersedes it. A toolkit migration is not part of this work.

## Required behavior

- Distinct pairing, authenticated source library and stream screens, with
  portrait/landscape layouts, accessible controls and keyboard-aware text entry.
- Controls derive from host permissions and capability descriptors. Missing
  sharing scope remains unknown; the current host truthfully advertises full
  display capture and shared-desktop input even for a window selection.
- A first rendered MediaCodec frame establishes live playback. Authentication,
  encoded geometry and a connected media socket alone do not.
- Primary tap/click, held-button drag, long-press secondary click, two-finger
  scroll and distinct HID/committed-text paths.
- Mapping rejects letterboxes/outside points. Gesture ownership is stable;
  pinch and third-finger input cancel remote recognition.
- Input commands retain original connection/selection/geometry/ownership tokens.
  Background, cancellation, geometry/target changes, disconnect and permission
  loss release held state and invalidate queued commands. Resuming does not
  automatically restore local control.
- A target change uses fresh authenticated control/media sockets because v1
  media records do not identify their target. Old frames cannot become the new
  target's live picture. Reconnect restores the selected target with control off.

## Existing work to reuse

Issues [#34](https://github.com/goncalompontes/vista/issues/34),
[#75](https://github.com/goncalompontes/vista/issues/75),
[#7](https://github.com/goncalompontes/vista/issues/7) and
[#71](https://github.com/goncalompontes/vista/issues/71) guide the milestone.
PR [#80](https://github.com/goncalompontes/vista/pull/80)'s portable reducer
semantics and PR [#85](https://github.com/goncalompontes/vista/pull/85)'s geometry
and ownership rules inform the native implementation. Android's design-fixture
tests cover the supported v1 input/media scenarios; they do not claim the future
clipboard, managed-session or enrollment cases are implemented. PR
[#102](https://github.com/goncalompontes/vista/pull/102) owns reference-client
semantic-action verification and stays separate.

## Verification

The JVM suite covers state/permission/stale-context behavior, geometry, gesture
precedence, shared wire fixtures and the supported design-fixture subset. The
opt-in Android instrumented check exercises a real host and a disposable GTK
target, including native effects and prompt modifier release. It also covers
background/resume, portrait/landscape, fresh connections on source changes and
automatic pairing/media recovery after host restart.

The host uses a native uinput absolute pointer for positions/buttons/wheel;
`ydotool --absolute` was found to apply accelerated relative motion and miss
requested coordinates. Keyboard/text continue through the private ydotool daemon.
No desktop acceleration settings are changed. Current runtime evidence is a
single-output Niri desktop; multi-output mapping needs separate validation.
Rust, Swift, Android app and instrumentation builds provide the relevant gates.

Physical-device testing and iOS emulation are not gates. Audio, clipboard,
discovery, independent sessions and TLS/WebRTC/protobuf migration remain
separate milestones.
