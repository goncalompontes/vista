# Review scenarios, open decisions and implementation sequence

Status: architecture review checklist. These are design expectations, not executed integration tests.

## Contract walkthrough

| Scenario | Required behavior | Owner / document |
| --- | --- | --- |
| Spoofed nearby hostname | Never paired by discovery; fail pin check before sending secret | Identity / lifecycle-security |
| QR expired or reused | No trust entry; bounded rejection and deliberate new pairing | Pairing / lifecycle-security |
| Paired host, remembered viewing, portal denies | Metadata only if allowed; no pixels, no desktop fallback | Policy + capture |
| View-only client requests input | permission_required; no native injection | Policy / protocol |
| Window grant with shared-seat-only adapter | Require explicit shared-desktop scope or remain viewing-only | Policy + input adapter |
| Three-finger swipe on Linux without overview | No overview binding/control; no leaked click/scroll | Client / client-experience |
| User remaps gesture | Override host suggestion except OS/accessibility-reserved gestures | Client / protocol |
| Sill-style pinch on picture | Local video zoom; inverse pointer mapping; no host resize | Client / inventory |
| Aa-like fit request unavailable | Explain/omit host resize; local zoom still works | Client + capability provider |
| Focus/app changes with menu open | Revision update; old invocation rejected, never chosen by index | Menu adapter / protocol |
| Resize during drag | Cancel held input; new geometry epoch; resume after matching frame | Input + media |
| Window closes and numeric ID reused | Old generation invalid, target_gone; no injection into replacement | Catalog + policy |
| Network loss while modifier held | Immediate local cleanup; host watchdog releases; no replay | Lease manager |
| Close/launch/paste response lost | Show outcome_unknown; refresh state; no automatic retry | Client + action service |
| Host revokes video during encoder work | Invalidate epochs, cancel/flush queues, blank client when notified | Policy + media |
| Client backgrounds | Release control, suspend media/clipboard; fresh approval on new session | Client lifecycle |
| Second controller requests desktop | busy until explicit transfer; never merge held states | Host lease manager |
| Host key changes | Stop and require explicit re-pair | Identity |
| Clipboard write allowed, input denied | Clipboard transfer possible, Paste Now denied | Policy |
| Backend advertises system-only audio | Explicitly label scope; never claim selected-app audio | Capability provider |
| Opt-in snapshot then forget host | Delete local image and keys; revoke host trust when reachable separately | Client |
| Independent session link lost | Revoke input; grace timer; supervised teardown after policy expiry | Supervisor |
| Compositor/host restart | New generations/epochs, reconcile managed processes, no stale session attach | Catalog + supervisor |
| Old client sees unknown extension | Ignore optional extension; reject required incompatible feature | Protocol |

## Example fixture coverage

fixtures.json provides a capability snapshot, action request/result and negative decision cases. A JSON parser can check syntax; meaningful conformance requires Rust/Swift schema validation and fake adapters during implementation. No current host accepts these fixture messages.

Future fake-host fixtures should script pairing approval/denial, catalog pagination/deltas, dynamic permissions, stale menus, backend failure, media pressure, resize, disconnect and alternate backend labels. Fake input adapter records effects instead of injecting into the desktop.

## Technical proposals needing implementation review

| Topic | Current draft | Review gate |
| --- | --- | --- |
| Media transport | Compare separate TLS video with WebRTC | Native interoperability and loss/latency/resource spike |
| Pairing | QR pin + one-use secret + host approval + later mTLS | Keychain/Rust TLS proof, pinning, revocation and cryptographic review |
| Friendly manual pairing | Full fingerprint/secret first | Select reviewed short-code mechanism before shipping short codes |
| Input liveness | 2 s heartbeat, 6 s lease expiry | Stall/disconnect tests; optimize without stuck-state risk |
| Managed disconnect grace | 60 s then teardown | UX review of unsaved work; configurable persistence later |
| Bounds and input delivery | Bounded reliable ordered input first | Responsiveness/loss/backpressure measurements |
| Independent compositor | No selection yet | #5 benchmark and scope/security evidence |

## Product decisions

Confirmed D1–D5 are recorded in README.md. Do not revisit them without new evidence or maintainer steering. Next product decisions, when their implementation becomes relevant: optional unattended-control policy; persistence of independent apps; whether public-internet relay infrastructure is desirable. They do not block the current design package and are not silently enabled.

## Implementation order

1. Review this architecture package and refine schema/error details. Keep design approval separate from code verification.
2. Generate versioned schemas and shared Rust/Swift fixtures. Implement fake host/adapters and policy checks without desktop access.
3. Adapt Sill connection/library/stream components with fake capabilities; verify native layouts, accessibility, focus and keyboard cancellation.
4. Implement host identity/pairing and control connection; negative tests before exposing any actions.
5. Integrate real catalog; validate capture through #2/#6; run transport spike and negotiate first video.
6. Integrate permission-scoped pointer/keyboard/text/actions with host-visible revoke and shared-desktop labels.
7. Add clipboard/audio/menus only where scoped capability evidence exists.
8. Prototype independent compositor/session supervisor separately under #5; hardening is a separate acceptance gate.

## Completion boundary

Architecture package is complete for review when links/fixtures parse, ownership and permission/epoch rules agree, source provenance is traceable, confirmed choices are recorded, and unknowns have gates. Issue #8 remains open through maintainer review; #3/#4/#5/#7 remain implementation work. Runtime capability, emulation, capture and latency claims require actual execution.
