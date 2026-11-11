# Vista native client experience — implementation-ready interaction contract

Status: **design-only candidate** (2026-11-09), no new UI code or simulator/device verification. Builds on [client experience](client-experience.md), [Sill feature inventory](sill-inventory.md), issue #7 (host capabilities), #4 (iOS), #34 (Android/Skip), #56 (wire), #59 (security), and proposed ADR #67. Conformance fixtures: [client-state-fixtures.json](client-state-fixtures.json).

## Product invariant

**One client interaction model regardless of host OS, Linux compositor or display server.** Host names/types may be shown for diagnostics, but the UI's action availability, scope, grant and lifecycle behavior derives only from portable capability/permission/session descriptors. Never write `if host == Niri/GNOME/KDE/Windows` branches to produce controls or infer permissions. Sill's visual structure and ergonomics are references; Vista is not a pixel-by-pixel Mac UX clone or an assumed code fork.

Client is allowed to own view-state: zoom/pan, language/accessibility, software keyboard visibility, local gestures, saved hosts, screen layout. Host owns authorization/availability/scope, session state and semantic action outcomes. The client may optimistically animate local actions, but may not display "remote action succeeded" until the host confirms or a state snapshot reconciles it.

## Source of truth and state model

```text
AppModel
  HostDirectory { saved, nearby, browsing_state, local_network_permission }
  TrustStoreView { pairing_state, verified_host_id, trust_warning }
  Connection { disconnected | resolving | tls_verified | authenticating
               | signaling | rtc_connecting | control_syncing | connected }
  Catalog { revision, items, picker_available, launchables, managed_sessions }
  Capabilities { revision, by_target, actual_input_scope, reasons }
  Grants { permission_revision, by_target, portal_state, control_lease }
  Stream { source_state, media_state, first_frame, geometry_epoch,
           decoder_generation, audio_state, quality_stats }
  UI { selected_host, selected_target, layout, zoom, pan, keyboard,
       input_mode, overlay, gesture_bindings, accessibility }
```

State dimensions are independent. **Connected** means authenticated control channel, **Streaming** means at least one decoded current-session video frame, **Controllable** means actual InputProvider grant + unexpired lease, **Clipboard enabled** means separate direction-specific grant. Do not collapse these into one "connected/live" boolean. Upon disconnect, remote actions/input/clipboard immediately disable, even if the last frame remains displayed as a labelled frozen preview.

Data flow: native WebRTC/protobuf adapter -> validated `HostEvent` -> pure reducer -> `AppModel` -> capability-driven SwiftUI/Compose view model. UI user intent -> pure intent router -> local action **or** host action/input message, never an OS-specific client shortcut. Each event has `connection_id` and relevant target generation/revision; reducer drops out-of-generation events and requests resync on gaps.

## Navigation and screen composition

1. **Hosts:** Saved hosts first, nearby discovery below, Add manually, pairing entry, verified/offline/untrusted status. Network availability and trust separate. An untrusted advertised host never shows a private catalog.
2. **Pairing:** QR scanner, manual address + full pin + long secret, fingerprint verification, host-approval pending, denial/expired/mismatch. A device label cannot become trusted without confirmed host approval + possession of client key. Settings can forget local host; remotely revoke is a separate host action.
3. **Library:** Tab/sections for **Active Windows**, **Choose Window** (portal picker), **Launch Apps**, **Managed Sessions**. Items show provenance/status, not fictional thumbnails. Displaying catalog requires explicit permission; picker-only flow does not impersonate a particular catalog window.
4. **Capture consent:** Explain target kind and active-desktop privacy. Native chooser runs on host, not a fake iPhone modal. Waiting/denied/cancelled/returned-different-kind states distinct. A selected portal source is a fresh stream target; only show catalog name if verified association.
5. **Stream:** Picture-first, compact status strip, supported host actions, input-mode control, keyboard, settings, app drawer/window switcher and explicit Share/Control grants. Always reachable stop/disconnect and input release controls.
6. **Permissions:** Video, audio, shared desktop input, clipboard-read, clipboard-write, desktop actions are separate toggles/requests. Actual input scope is visible, especially **Controls shared desktop** vs **Controls private session**.
7. **Managed session:** Show app launch queued/starting/active/stopping/failed, session owns target, explicit Stop app, crash cleanup status and **display separation is not a filesystem sandbox** explanation.
8. **Diagnostics/Recovery:** Separate state for transport offline, signaling/auth failure, permission lost, target closed, decoder waiting keyframe, congestion, app crashed, reconnecting. Suggested remediation must follow structured error code, no guessed success.

### Suggested compact wireframes (semantic, not pixel design)

```text
[Hosts]                             [Library]
Vista                 Add Host      Verified Host   [Disconnect]
Saved                             Windows (metadata if authorized)
  ✓ Laptop • last connected         Editor           [Open]
Nearby                              Browser          [Open]
  ? Unknown host (untrusted)       Choose a window    [Select on Host]
                                Launch an app
                                  IDE / Browser   [Launch]
                                Managed sessions
                                  Isolated App     [Attach] [Stop]

[Stream, iPhone portrait]          [Stream, iPad wide]
[Stop] [Host/source] [Status]       [Sidebar apps] [Video ................]
[VIDEO content / letterbox]        [Sidebar ... ] [Video ................]
[Keyboard] [Trackpad] [Actions]    [Actions] [Input] [Permissions] [Stats]
[Thumbnail/source strip]           [Keyboard/trackpad panel when requested]
[Input scope: shared desktop]
```

No important action may be accessible *only* via a gesture, hover, or a control that disappears behind the software keyboard. Unknown action descriptors are safely hidden or disabled with explanation; never map an unknown action to a guessed shortcut.

## Sill interaction parity and divergence

Use pinned upstream inventory in `docs/design/sill-inventory.md` and trace each Sill control to one of four owners:
- **Client-local:** zoom, pan, picture layout, keyboard toggle, navigation, reconnect UI, gesture editor, thumbnail strip presentation.
- **Portable input:** physical HID keys, committed text, pointer absolute/relative motion, scroll with unit/phase, button and touch events, release-all.
- **Semantic action:** focus, close, maximize (only if real provider), next workspace, launcher, application menus, rename/stop managed session.
- **Host optional:** native application menu tree, text-size adjustment, clipboard, audio, display remapping, multiple windows, app/session launch. Mac-only Spotlight/Menu Bar/Command-shortcut assumptions are never universal.
  
**Parity statuses:** `documented_upstream`, `mapped_in_design`, `mock_verified`, `native_iOS_verified`, `Android_verified`, `host_verified`, `intentionally_not_supported`. The existing inventory is not an exhaustive feature certification; track each interaction's source commit, Sill file, screenshot/test, Vista target capability, backend evidence and current verification. Reused implementation source must respect upstream notices/license (#73).

## Input ownership and gesture precedence

Priority: (1) OS-required/system gestures, safe-area and accessibility, (2) active native controls/keyboard/VoiceOver, (3) local video zoom/pan, (4) explicitly configured semantic action gesture, (5) portable pointer/touch gesture to host. No gesture should both zoom locally and inject remote scroll/touch. Recognizers are mutually exclusive after a clear decision point; if cancelled mid-gesture, release any host buttons/contacts.

Default interaction policy candidate (user-configurable, tested against system gesture conflicts):

| Gesture/control | Default local intention | Host request only if |
| --- | --- | --- |
| Single tap video while controlling | Primary click at mapped location (or trackpad tap mode) | Valid fresh input lease and supported pointer scope |
| Long press / secondary click | Show native interaction choice or secondary click | Host button capability supported, no system selection gesture conflict |
| One-finger drag in trackpad mode | Relative pointer motion | Valid lease; suppress if local overlay active |
| One-finger drag in direct mode | Absolute move or true touch contact | Host advertises genuine touch/absolute scope and matching geometry |
| Two-finger move | Scroll, with unit and phase | Gesture assigned to remote scroll and no local zoom ownership |
| Pinch on picture | **Local** zoom/pan, not host desktop pinch | Never reinterpret as remote pinch unless explicit capability/setting |
| Dedicated keyboard button | Show system keyboard; physical keys vs text commit distinguished | Input grant active; view-only keyboard is disabled with hint |
| Supported host action chip | Typed semantic `ActionRequest` | Action currently supported+enabled+authorized and target/revision fresh |
| Copy/paste chip | Explicit clipboard flow | Independent directional grant; platform paste permission fulfilled |

A user must be able to view the active binding and restore defaults. Reserve system swipes and accessibility gestures; do not override OS-wide iPad trackpad gestures. Host can **suggest** mappings, but client accessibility/system safety rules and user overrides take precedence; unseen host actions never become reserved gestures.

### Geometry and coordinate correctness

Define source pixel geometry (crop, orientation, pixel aspect, safe-area presentation), rendered letterboxed image rectangle, local viewport zoom/translation and monotonic `geometry_epoch`. Input coordinates invert **all** client transforms in the order applied: view coordinate -> displayed video rectangle -> inverse pan/scale -> inverse orientation -> source normalized coordinate [0,1]. Reject taps outside the actual content rectangle (e.g. black letterboxes); never clamp an outside tap onto the nearest edge if it could trigger an unintended destructive click. On resize/rotation the client pauses absolute input, waits for fresh geometry epoch and reprojected view, then resumes. Avoid sync-sensitive forced-frame change without updating input mapping.

Unit-test mapping for portrait/landscape/ultrawide/4:3, letterbox/pillarbox, zoom 1x–5x, pan boundaries, iPad split-screen, device rotation, high DPI, video resize and stale geometry. Do not send NaN/Infinity or coordinates outside valid source area. The host must revalidate geometry epoch too.

### Physical keyboard vs text/IME

HID physical usages remain distinct from composed text. Hardware keyboard shortcuts are not indiscriminately reinterpreted as Mac Command; the host capability registry defines semantic alternatives. Sticky modifiers and key-up follow the input lease; on background/disconnect, cancel and release-all. IME marked-text composition should have a separate begin/update/commit/cancel model **if** supported by host input protocol; current draft has `CommittedText` only, so do not claim full IME until #56 introduces composition event messages or a documented supported platform integration. Autocorrect, repeat, accessibility keyboard commands and local navigation must not send unintended remote events.

## Consent and safety affordances

- A viewer-only video session is still useful and labelled clearly; requesting control triggers scope-specific consent. A user cannot infer private window control from viewing a window.
- Window picker selection/confirmation: show `Selected on host` and its actual granted type. If identity matching is unverified, do not display "Editor window" based on prior catalog suggestion.
- When host portal denied input or source capture, do not retry permission prompts automatically; provide a deliberate `Try again` action with reason.
- Clipboard transfer requires user intent. On iOS prefer the system-supported explicit paste control where appropriate (e.g., UIKit UIPasteControl) rather than silent pasteboard polling. Show direction, target and size. Files not supported until a separately reviewed permission/transfer feature.
- Audio mute affects local playback by default; a separate host source mute action requires its own capability. Do not claim audio is private per app if provider is system mix.
- Destructive host actions (close shared window, stop session, revoke device) require scope-specific confirmation and display actual effect.
- Remembered UI previews are opt-in/local-only, encrypted or cleared consistent with platform storage policy; forgetting a host clears local snapshots and keys. No secret/title/clipboard in diagnostic logs.
- Background/resume: stop input ownership immediately, release held states and reflect stream policy; do not automatically re-request revoked native grants when foregrounded.

## Responsive and accessibility requirements

iPhone portrait: video first, status and stop reachable, compact overlay/trackpad, bottom action strip above system keyboard. iPhone landscape: fullscreen-first with hideable edge controls; permanently accessible disconnect gesture/button. iPad wide: source sidebar + sizable video + optional attached hardware-keyboard/trackpad shortcuts. iPad compact/split view: recompute from available scene bounds/safe areas, not device model or equal left/right insets. Android: Jetpack Compose Material patterns where native rather than reproducing iOS chrome exactly, while preserving wire behavior and state semantics.

Accessibility: VoiceOver/TalkBack labels and hints include source scope, permission state, action consequence, and live-view vs frozen-preview distinction; Dynamic Type, sufficient hit targets, contrast, reduce motion, keyboard navigation, external switches and focus restoration after modal/portal waiting. All essential actions remain discoverable without custom gestures. Native iOS/Android paste/privacy prompts are respected; no silent clipboard sync.

## Pure reducer rules (testable with fake hosts)

**State events:** `DiscoveredHost`, `HostAuthenticated`, `PairingPending`, `PairingRejected`, `CatalogSnapshot`, `CatalogDelta`, `CapabilitySnapshot`, `PermissionChanged`, `CaptureChooserOpened`, `CaptureApproved`, `MediaFirstFrame`, `GeometryChanged`, `LeaseAcquired`, `LeaseRevoked`, `TargetClosed`, `ConnectionLost`, `ReconnectStarted`, `ReconnectAuthorized`, `SessionStopped`.

**Intent outputs:** `SelectHost`, `StartPairing`, `OpenPicker`, `OpenTarget`, `RequestControl`, `ReleaseControl`, `InvokeAction`, `SendInput`, `TransferClipboard`, `MuteLocalAudio`, `StopManagedSession`, `ForgetHost`. The reducer either emits local UI mutation or a *validated candidate* request; host still authorizes all effects. Every event is scoped to an active connection ID, target generation and relevant revision.

Deterministic invariants:
1. Connection not authenticated => no catalog UI data or host control intents emitted.
2. Media first frame absent => status cannot say `Streaming`.
3. Video grant exists but input grant absent => controls disabled as `View-only`.
4. Input grant allowed but lease expired/revoked => no input is emitted and held-state cleanup requested.
5. Backend capability disappears => action control disappears/disables immediately; no retry against stale revision.
6. Picker-selected stream with unverified catalogue binding => no fabricated catalog identity; source label remains generic.
7. New connection epoch => clear pending action results, suppress replay, resubscribe snapshots, request keyframe before declaring recovered video.
8. Background, decoder restart and geometry change cannot convert an input event into a different target.
9. Clipboard read/write require separate grants and explicit source and sink; no passive polling.
10. User cancels consent => no media/input activity; no hidden monitor fallback.

## Worker implementation slices

- **UX1**: Reuse existing `client/ios/Vista/Views/{WelcomeView,LibraryView,StreamPreviewView,VideoStreamView}` and `client/core/Sources/VistaCore/{Capabilities,CapabilitySession,Models}.swift` as a starting point; move demo behavior behind explicit preview mode. Add pure state model + fake event reducer with fixture tests.
- **UX2**: Implement hosts + verified pairing + catalog/picker-only surfaces and permission gates with generated protobuf (#56/#59), without live pixels.
- **UX3**: Plug native WebRTC video and reconnection state (#58); render first-frame, decoder wait, source resize and frozen preview honestly.
- **UX4**: Implement gesture/input router and geometry inverse transform; connect physical keyboard/text/scroll and release-all (#60) with lease/grant visibility.
- **UX5**: Add capability actions and menus, independent audio/clipboard permissions, managed session launcher UI and explicit input-scope warning (#61/#62).
- **UX6**: Android/Skip parity: compile and run generated protocol fixture/state reducer, platform-specific storage/WebRTC/MediaCodec input/clipboard implementations; no handwritten independent wire mirror.
- **UX7**: Accessibility/native-device validation on iPhone portrait/landscape, iPad split view, hardware keyboard/trackpad, Android emulator/device. Sill parity evidence table updated by source commit, not by aesthetic similarity alone.

## Acceptance matrix (no code execution implied)

- Fake Niri provider and synthetic GNOME/KDE provider produce identical UI for the same capabilities, regardless of OS-name string.
- Picker-only client with no compositor IPC can request and display a consented window source.
- View-only stream shows real decoded frames but blocks keys, pointer and clipboard actions without grant.
- Granted input shows accurate scope badge; lease expiry removes controls and releases held input.
- Catalog and capability revision gaps force resync; stale target/action result does not restore a revoked control.
- Stream orientation/letterboxing/zoom mapping is correct and safe at boundary.
- Disconnect/cert mismatch/pairing expiry/portal refusal/managed-app crash all have truthful, accessible recovery states.
- iOS and Android fixtures render equivalent semantics with platform-native controls; no mock "connected" label in live mode.
- The Sill inventory shows each original interaction as verified/mapped/deliberately deferred, with no incorrect Mac-only semantics smuggled into Linux.

## References

- Existing Vista native UX and Sill inventory: [client-experience.md](client-experience.md), [sill-inventory.md](sill-inventory.md)
- Apple pointing device guidance: https://developer.apple.com/design/human-interface-guidelines/pointing-devices
- Apple explicit paste control: https://developer.apple.com/documentation/uikit/uipastecontrol
- Apple SwiftUI geometry: https://developer.apple.com/documentation/SwiftUI/GeometryReader
