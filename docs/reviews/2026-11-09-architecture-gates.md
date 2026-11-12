# Design review 2026-11-09 — Vista platform-neutral host, protocol, security and licensing

Status: **architectural review performed; technical approval recommended with conditions**. Proposed ADR 0002 in PR #67 and the stacked design package PR #76 remain unmerged. The review does not certify security, generated schema, third-party licensing or real desktop compatibility.

Historical record: this review is a snapshot of 2026-11-09. The licensing gate it names was decided on 2026-11-12 in [ADR 0004](../adr/0004-license-and-niri-ipc-policy.md) (Apache-2.0, protocol-only Niri adapter); issue #73 remains open for provenance inventory and CI enforcement. The findings below are kept as written.

## Scope and review approach

Reviewed `docs/architecture-target.md`, `docs/adr/0002-host-platform-neutral-core.md`, `docs/design/{provider-interface-contracts,linux-desktop-compatibility,canonical-protocol-spec,security-threat-model,adapter-library-evaluation,client-experience-implementation-spec}.md`, the proto candidate, issue backlog and public upstream docs for Niri IPC, TLS/mTLS/Apple/Android identities, Protobuf and Apache/GPL compatibility. Compared consumer/provider boundaries against current `host/src/{catalog,capability,capture,lifecycle,live,protocol,transport}.rs` filenames and dependency manifest, **without compiling or running Vista**.

The .proto file has undergone a basic in-memory token audit for duplicate fields/tag numbers and enum symbol collisions (75 declaration blocks in the inspected pre-review candidate, 58 message declarations and 17 enum declarations; no duplicates detected by that simple check). It **has not been accepted by protoc**, compiled to Swift/Rust/Java, validated with Buf, or used in a real protocol exchange. An automated tool smoke test remains mandatory.

## Review result by subsystem

| Subsystem | Review result | Conditions/remaining proof |
| --- | --- | --- |
| Portable HostService and independent providers | **Accept as implementation architecture proposal** | Enforce no OS handles in domain/wire; provider registry is per-capability, not one mandatory Niri adapter; mock-provider conformance in #68 |
| Portal picker and source identity | **Accept semantic boundary** | Picker source IDs != catalog targets without trustworthy binding; no monitor fallback; run consent and stale-generation fixtures #71 |
| Independent app runtime | **Accept extension point only** | Managed compositor has separate capture/input providers; no assumed login-session portal; private socket != OS sandbox; benchmark #70/#62 |
| WebRTC + generated protobuf | **Accept direction, schema provisional** | Verify webrtcbin/webrtcsink, protobuf compiles, channel semantics, reconnect and revocation on iOS/Android #56/#58 |
| Authentication and key lifecycle | **Approve ADR 0003 as preferred spike candidate, not production security** | CSR signature and iOS SecIdentity/Android KeyManager must work, mTLS/host pin+DTLS fingerprint binding tested; exact cert lifetime/revocation policy review #59/#77 |
| License and Niri adapter | **Recommend Apache-2.0 + own JSON socket adapter** | Maintainer must approve license (#73); upstream Niri public JSON documented but implementation independence and dependencies checked |
| Linux desktop support | **Accept capability-evidence policy** | Actual Niri/GNOME/KDE/wlroots and future X11 tests; probe-only never equals hardware verified #69/#72 |
| Client interaction layer | **Accept reducer/gesture separation** | 23 fake-host scenarios plus native device tests #74/#75; OS-name never drives action semantics |

## Critical findings and dispositions

### F1 — Niri adapter selection must not silently lock license or architecture (high)

Earlier issue #57 and target architecture preferred official `niri-ipc` crate. It inherits the niri GPL-3.0-or-later workspace license, and the project's root has no LICENSE in the inspected branch. Niri's public IPC is a documented line-delimited JSON request/reply over `$NIRI_SOCKET`, with event-stream and backwards-compatible JSON field policy. **Preferred conditional direction:** independently authored async Unix socket JSON adapter using `serde` plus minimal typed domain translation, separate event/reply sockets; no `niri msg` subprocess per action and no copied GPL source. This respects portable host boundaries and avoids requiring the GPL crate if Apache-2.0 chosen. **Not** a legal determination that protocol compatibility is never subject to IP claims. See proposed ADR 0004 and #73.

### F2 — Initial mTLS enrollment had no identity bootstrapping (high)

A client cannot use a certificate it has not enrolled; a public-key field is not authentication. Proposed ADR 0003 splits QR-pinned anonymous *restricted enrollment* TLS from certificate-authenticated *signaling* TLS. Use PKCS#10 CSR proof, host-issued constrained client cert after local approval, then normal mTLS. Proto candidate field updated from raw public key to signed CSR DER and return cert/CA DER on approval. This is not proof that `SecIdentity` on iOS and `AndroidKeyStore/X509KeyManager` will interoperate; #77 is a blocking spike.

### F3 — WebRTC signaling identity must bind to media identity (high)

Transport security only protects the real peer if authenticated signaling's SDP DTLS fingerprint is bound to the verified host/client identity and selected session. Require negative SDP substitution and revoked-mid-handshake tests; no media or input before verification. Reject TLS 0-RTT application commands. This is standard WebRTC security behavior to configure/test, not reason to create custom media encryption.

### F4 — Schema candidate needs a real compiler and semantic validator (high)

Basic token structural audit showed no duplicate field or enum symbols, but does not imply syntax/type validity, cross-language API, safe limits, or a canonical wire format. Need actual `protoc`/Buf, deterministic golden fixtures generated from code, Rust/Swift/Android decoding and negative tests. Use protobuf binary messages directly on SCTP data channels rather than custom length framing. Domain validator must reject enum zero/unknown when used for authorization, stale epochs, wrong input scope and invalid geometry. Do not promote `docs/design/wire/vista-v1.draft.proto` into `proto/` before compilation and security decisions.

### F5 — Privacy boundary between catalog and capture (high)

Portal grant for a selected window is not an opaque compositor window ID. Treat uncorrelated portal sources as new picker-selected targets. If WINDOW requested and portal returns MONITOR, close/reject; never silently widen to screen. Shared desktop input is potentially seat-wide even when video captures one window; disclose actual input scope and support view-only mode. Contract #71 already specifies the tests; enforce against fake-host and live portal checks.

### F6 — Async provider interface sketches are not compiled Rust (medium)

Current `async fn` trait sketches are illustrations, not guaranteed object-safe `dyn Trait` interfaces on stable Rust. Worker #68 should decide `async_trait`, boxed futures, generics/enum dispatch or static lifecycle handles after measured needs. Do not prematurely split into many crates; first enforce import/dependency boundaries inside existing workspace. Keep GStreamer/PipeWire frame ingest opaque and efficient—core-neutral types must not force a CPU copy.

### F7 — Client status model and input edge reliability need conformance (medium)

Connected != paired != first decoded frame != input-authorized. Input button/key edges must be reliable, whereas optional lossy motion may live on another channel; design independent per-channel sequence domains and release-all. Absolute touch must use fresh geometry epoch after orientation/zoom changes. #74/#75 fake-host fixtures and iOS/Android native tests should gate UX claims.

### F8 — Documented upstream capability != verified Vista support (medium)

Portal backend README/version may claim ScreenCast WINDOW, RemoteDesktop, Clipboard or Weston PipeWire, but only opt-in tests prove consent/scope/input or low-latency capture for specific compositor versions. Maintain the per-capability matrix; don't infer from OS name or package presence.

## Approved architectural invariants for implementers (subject to PR acceptance)

1. Every host runtime is a composition of independently present providers, not an OS/compositor enum with mandatory methods.
2. Host policy, identity, capabilities, target generations, grant/lease lifecycle and media transport are independent from Linux-specific handles.
3. An enumerated catalog entry never grants capture; a portal approved source never proves correspondence to a catalog entry.
4. Source kind/scope and native input target are explicit; no silent broader capture or privileged injection fallback.
5. A managed app uses session-owned capture/input; shared desktop portal is not silently reused. Display isolation is not a filesystem/network sandbox.
6. One generated protobuf schema and one standard authenticated WebRTC production transport; legacy custom channels remain diagnostic until gated retirement.
7. Unpaired/unauthorized clients get no metadata/pixels/actions; revocation invalidates effects before cleanup and releases held inputs.
8. Clients render supported/enabled/granted actions from descriptors, never based on Niri/GNOME/KDE/macOS labels.
9. No unapproved GPL crate is added solely for ergonomic typed IPC; use maintainer-approved dependency licensing.
10. Hardware support/performance claims require versioned reproducible evidence.

## Explicitly *not* approved by technical review alone

- Selecting/relicensing Vista under Apache-2.0, GPL or dual license: #73 owner approval.
- Claiming mTLS certificate enrollment is deployed or secure: #77 native TLS/CSR tests, #59 security audit.
- Treating `.proto` as canonical or generated wire code as buildable: #56 compiler and cross-language gate.
- Confirming GNOME/KDE/Niri/wlroots/X11 per-window capture or true per-window input without tests.
- Merging PR #67 or #76 automatically. They are reviewable proposals; after approved, merge #67 first and restack/review #76.
- Closing release/implementation issues based solely on this design review.

## Prioritized next actions

| Order | Action | Owner gate | Exit evidence |
| --- | --- | --- | --- |
| 1 | Review/merge portable host architecture #67 and reconcile typed-vs-JSON Niri choice | Maintainer (#73)/architect | Accepted ADR 0002 + license policy |
| 2 | Build isolated PKCS#10-to-client-cert TLS interoperability spike | Worker #77 | Rust pinned TLS + iOS SecIdentity + Android Keystore handshake tests |
| 3 | Compile candidate proto and generate/verify binary fixtures in Rust/Swift/Android | Worker #56 | Clean protoc/Buf, version vectors, no generated drift |
| 4 | Choose/codify project LICENSE and third-party notices | Maintainer #73 | Root LICENSE, Sill and dependency notices after consent |
| 5 | Run fake-provider contracts and portal capability/scope probes | Workers #68/#69/#71 | No Niri dependency for picker stream; permission and restart scenarios |
| 6 | Validate synthetic WebRTC then consented real source with authorized signaling | Workers #58/#59 | Real device playback, revocation and reconnect evidence |
| 7 | Client reducer, gesture/geometry and permissions mock tests | Workers #74/#75 | All 23 state fixtures and native device acceptance |

## Sources

- Niri IPC: https://niri-wm.github.io/niri/IPC.html
- Niri license: https://github.com/niri-wm/niri/blob/main/Cargo.toml
- Apache/GPL direction: https://apache.org/licenses/GPL-compatibility.html
- Rustls client verification: https://docs.rs/rustls/latest/rustls/server/struct.WebPkiClientVerifier.html
- Apple identities: https://developer.apple.com/documentation/security/identities
- Android Keystore keys/certs: https://developer.android.com/reference/android/security/keystore/KeyGenParameterSpec
- Protobuf syntax/enum scoping: https://protobuf.dev/programming-guides/style/
- WebRTC security: https://www.rfc-editor.org/rfc/rfc8827 ; https://www.rfc-editor.org/rfc/rfc8842
