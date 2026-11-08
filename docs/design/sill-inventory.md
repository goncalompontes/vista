# Sill interaction inventory and Vista adaptation

Source snapshot: [Saffsanity/sill 41e8cb8801f8e9d387e27a0d498dee193d17e9ec](https://github.com/Saffsanity/sill/tree/41e8cb8801f8e9d387e27a0d498dee193d17e9ec).
Evidence is source/document inspection, not a hands-on parity certification. README describes product behavior; implementation-specific rows cite code. This inventory covers the documented user-facing feature families; an exhaustive event-by-event audit remains necessary while porting.

## Evidence

- [README](https://github.com/Saffsanity/sill/blob/41e8cb8801f8e9d387e27a0d498dee193d17e9ec/README.md): connection paths, pairing, media, menus, text size, virtual display and screenshots.
- [PhonePortraitLayout.swift](https://github.com/Saffsanity/sill/blob/41e8cb8801f8e9d387e27a0d498dee193d17e9ec/iOSClient/PhonePortraitLayout.swift): picture, controls, thumbnail strip, modifier row, trackpad and Larger Picture arrangement.
- [TrackpadGestures.swift](https://github.com/Saffsanity/sill/blob/41e8cb8801f8e9d387e27a0d498dee193d17e9ec/iOSClient/TrackpadGestures.swift): deterministic swipe/pinch/spread recognition, cancellation and suppression of stray pointer events.
- [StreamZoom.swift](https://github.com/Saffsanity/sill/blob/41e8cb8801f8e9d387e27a0d498dee193d17e9ec/iOSClient/StreamZoom.swift): local 1–5x picture zoom and inverse input mapping.
- [HardwareKeyboardPolicy.swift](https://github.com/Saffsanity/sill/blob/41e8cb8801f8e9d387e27a0d498dee193d17e9ec/iOSClient/HardwareKeyboardPolicy.swift): hardware-keyboard ownership and scroll anchoring.
- [KeyChords.swift](https://github.com/Saffsanity/sill/blob/41e8cb8801f8e9d387e27a0d498dee193d17e9ec/iOSClient/KeyChords.swift): Mac-specific Spotlight/paste shortcuts, held-key and modifier cleanup.
- [ClipboardChip.swift](https://github.com/Saffsanity/sill/blob/41e8cb8801f8e9d387e27a0d498dee193d17e9ec/iOSClient/ClipboardChip.swift): explicit iOS paste interaction.
- [MacMenuElements.swift](https://github.com/Saffsanity/sill/blob/41e8cb8801f8e9d387e27a0d498dee193d17e9ec/iOSClient/MacMenuElements.swift): lazy remote menus, disabled rows, revision-aware invocation.

## Inventory

Owners: L = client-local; I = portable input; A = semantic host action; C = optional host/media capability.

| Feature family / behavior | Owner | Vista disposition | Present Vista behavior |
| --- | --- | --- | --- |
| Nearby and saved host tiles/list | L + C | Preserve structure; names describe hosts, not Macs | Welcome/demo only |
| Pairing QR / manual entry / forget device | L + C | Preserve flow; authenticated identity contract replaces Sill wire format | Absent |
| LAN, VPN, wired, direct wireless | C | LAN/Tailscale first; wired/direct wireless deferred | Absent |
| Last-session tile picture | L | Opt-in local retention (D5); clear on forgetting host | Absent |
| Window thumbnails and application drawer/search | L + C | Preserve patterns; catalog distinguishes windows from launchable apps | Basic sample gallery |
| Window focus/close/minimize/fullscreen | A | Individual capabilities; close window and terminate session are distinct | Absent |
| Whole desktop selection | C | Optional explicit source; never single-window fallback | Absent |
| Phone portrait picture + controls + trackpad | L | Closely adapt layout; host actions occupy supported slots | Generic preview |
| Larger Picture / keyboard-aware arrangement | L | Preserve intent; no promise to resize a host window | Absent |
| Two-finger picture pinch/pan, up to 5x | L | Preserve; transform input back to visible source coordinates | Absent |
| Tap/click, secondary click, drag/hold | I | Preserve; explicit button down/up and cancellation | Inactive labels |
| Touch/trackpad scroll and momentum | I | Preserve with specified units/phases; avoid applying acceleration twice | Absent |
| Pointer hover, mouse, Apple Pencil as pointer | I + C | Pointer role first; pressure/tilt require separate future capabilities | Absent |
| Host pointer position feedback | C | Target-scoped cursor updates; shared pointer clearly visible | Absent |
| Three/four-finger swipe/pinch/spread | L -> A | Client recognizes; negotiated semantic binding; no hardcoded Mission Control | Absent |
| Spotlight / Spaces / Exposé / Show Desktop | A | Optional launcher/workspace/overview/show-desktop actions; unavailable features omitted | Absent |
| Esc/Tab/arrows and held modifier strip | I | Physical HID keys; configurable labels; release on cancellation | Absent |
| Hardware keyboard ownership | L + I | Remote only with control grant and stream focus; local panels suspend it | Absent |
| Software keyboard, composition, dictation | L + I | Separate committed text from physical keys; do not send both | Absent |
| Bidirectional text clipboard and Paste Now | C + A | Independent directions; explicit iOS paste; Paste Now needs write + input grants | Absent |
| Per-app/system audio; local mute | C + L | Scope exposed honestly; local mute never changes host system volume | Absent |
| Remote application menus | C + A | Optional lazy tree with revisions; not guaranteed across Linux apps | Absent |
| Host settings / client settings / ACKs | A + L | Separate settings ownership and confirmed state | Absent |
| Aa text-size/window-resize control | A | Capability window.fit_viewport, explained as host geometry change; never claim font-size API | Absent |
| Move window to virtual display and restore | C + A | Optional relocated-window mode; NOT independent process/session isolation | Absent |
| Independent remote application | C | Vista addition; explicit managed session and security description | Sample metadata only |
| Tour, error/help states, quality pressure feedback | L + C | Contextual help and equivalent accessible controls | Minimal preview |
| Foldable posture arrangements | L | Defer specialized posture integration; responsive layouts first | Absent |

## Porting rules

Use the Sill screen structure as the reference; factor components away from its StreamClient before reuse. Keep pure gesture/layout logic testable independently. Replace Apple modifier bitfields and Mac-specific shortcut generation with the Vista contract. A missing backend feature must not leave an apparently working control.

Sill is Apache-2.0 licensed per its repository. Before copying source, verify that file's license/dependencies, retain applicable notices and license text, identify modifications and provenance, and review required NOTICE treatment. This design copies no implementation source and grants no claim of endorsement.

A later implementation audit must record exact copied files/revisions, validation status on iPhone/iPad, and any intentional behavioral difference. Source presence and documentation are not proof that every upstream feature works on every device.
