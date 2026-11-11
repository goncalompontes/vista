# Canonical Vista protocol v1 — design specification

Status: **design candidate, not implemented** (2026-11-09). Companion schema: [vista-v1.draft.proto](wire/vista-v1.draft.proto); binary fixture candidates: [golden-v1.json](wire/golden-v1.json). Complements #56 and #71, proposed ADR 0002 (#67), and the existing capability contract (#7). Existing `docs/protocol.md` describes **prototype JSON/PSK TCP**, not the future production WebRTC contract; do not delete it until migration #64.

## Normative goals and boundaries

1. **One portable schema, one domain meaning.** All client platforms use the same protobuf binary wire contract, independent of Linux, Wayland, Niri, PipeWire, Apple modifiers, Android/Skip and native handles. Wire types do not authorize native access; policy + grant + lease checks always run on the host.
2. **No custom transport protocol.** Authenticated TLS signaling and standard WebRTC media/data channels are chosen in #58/#59. This schema is payload/application semantics only; RTP, ICE, DTLS-SRTP, SDP, congestion and codecs remain library-owned.
3. **No mandatory compositor window enumeration.** Portal-picked sources have distinct identities from catalog windows until verified by a provider. A catalog ID cannot substitute for a capture grant.
4. **Versioned compatibility.** Preserve unknown protobuf fields in binary relays where possible; unknown *required feature identifiers* cause explicit rejection. Do not use ProtoJSON as the production carriage: unknown-field preservation is different in JSON and binary.
5. **No cross-language hand-maintained mirror.** The schema generates/feeds Rust `prost`, Swift `swift-protobuf`, and a proven Android Kotlin/Java generator. Verify Skip Lite interoperability; do not assume generated Swift is transpile-compatible.
6. **No irreversible input replay.** Every channel has an explicit lifetime and stale generation/permission/lease checks. Never apply input/actions simply because the data channel delivered bytes.

## File and generator policy

Initial package: `vista.v1`, schema candidate in `docs/design/wire/vista-v1.draft.proto`. After review move to `proto/vista/v1/` and commit pinned Buf module/lint/breaking configs, source schema, lockfiles and generation steps. Recommended tools to evaluate: `buf` for compatibility/lint, `prost` + `prost-build` for Rust, `swift-protobuf` for iOS, `protoc` Java/Kotlin output or a maintained Kotlin generator for Android. **No tooling version is selected by this document**, and no generated build was run. Validate generated SDK API compatibility and licensing in separate language CI before merging canonical schema.

Use `proto3` with `optional` only when scalar presence matters and `oneof` for mutually exclusive results. Enum zero values are always `*_UNSPECIFIED`; a new unknown enum numeric value must not be interpreted as privileged success. Never reuse removed field numbers; reserve numbers and names explicitly in the production schema. Changing tag type, oneof membership, presence semantics or enum meaning is breaking even where bytes happen to decode. Both schema evolution and application feature negotiation are required; use `buf breaking` against main and cross-language fixtures.

## Channel map and wire framing

| Channel | Existing standard transport | Payload | Ordering/reliability rule |
| --- | --- | --- | --- |
| Bootstrap/signaling | authenticated TLS endpoint (exact HTTP/WebSocket framing owned by chosen library) | `BootstrapEnvelope`, `RtcSignal` | request/response correlation; no catalog/control pre-auth |
| Domain control | one WebRTC reliable ordered SCTP data channel | `ControlEnvelope` | channel-local sequence, deduped requests, snapshot/revision convergence |
| Input edges | reliable ordered data channel | `InputEnvelope` for button/key/touch begin/end/cancel, text commit | no lost key-up; release-all on revoke/close |
| Pointer/touch motion | separate optional unordered/partially reliable data channel **only after measurements** | motion-only `InputEnvelope` | latest-generation-wins; never route button/key edges there |
| Clipboard data | independent reliable bounded data channel | `ClipboardChunk` | transfer ID, exact offsets, size/time limits; separately consented |
| Media | WebRTC video/audio tracks (RTP) | **no Vista media framing** | WebRTC codec/RTCP adaptation, request IDR on discontinuity |

Messages on each data channel are **one protobuf binary message per data-channel message**; data channels already preserve message boundaries. Do not add a custom TCP-style length prefix inside SCTP. Use `protobuf` for domain control, not for encoded H.264 NAL units. SDP/ICE body strings are signaling metadata passed to the standard WebRTC implementation after size checks and identity binding.

Proposed default per-message caps (review/measure; enforce before allocating): 64 KiB control message; 16 KiB for input events; 64 KiB signaling candidate and 256 KiB SDP; 64 KiB individual clipboard chunks, with 1 MiB *total text payload* in v1; 256 catalog items per page/snapshot; 256 capability descriptors per page/snapshot; 64 action arguments total and <=4 KiB of action text. Snapshot pagination must be added before any host with more entries is accepted; the v1 initial implementation may instead return a structured resource-exhausted error. Explicitly decide whether later clipboard file transfer gets its own gated feature; do not silently permit it.

## Identity and provenance

- Host fingerprint comes from the pinned asymmetric host identity, **not** from an arbitrary discovery result. `HostRef` is not a credential.
- Host-issued `TargetRef` = opaque ID + type + relevant managed session ID + monotonically advanced generation. Never send a compositor window number, PID, PipeWire node, X11 Window ID or Wayland object as the opaque ID with reversible encoding.
- `CaptureSelection` = independently generated selection target + approved capture grant + actual source kind. A `verified_catalog_target` exists **only** when trusted provider evidence binds chooser source to catalog entity. Title/PID/geometry matches are non-authoritative.
- `GrantRef` is a host-validated grant lookup reference bound server-side to principal, target, source provider, permission kind, epoch, scope and expiry; **not** a bearer token.
- `InputLease` is single-writer per controlled scope, tied to grant, principal and connection. Every side effect validates lease and scope, including auto-release on revoke, disconnect and expiry.
- Changing backend generation, selected source, portal permission, or connection invalidates applicable refs; never replay an old action against a newly reused native window ID.

## Negotiation and state machines

### Bootstrap

```text
UntrustedDiscovered -> PinnedTlsConnected -> BootstrapHello -> VersionSelected
  -> (Enrolling -> AwaitLocalApproval -> Paired) OR
     (PreviouslyPaired -> ClientAuthVerified)
  -> AuthorizedSignaling -> RtcNegotiating -> DtlsFingerprintVerified
  -> DataChannelEstablished -> ControlHello -> SnapshotSynced
```

A new pairing enrollment requires a QR-supplied pin + one-time secret (see security spec). Reject after version mismatch before any authorization or media. Preferred pairing candidate is [ADR 0003](../adr/0003-device-identity-enrollment.md): a restricted QR-pinned TLS enrollment route verifies a **signed PKCS#10 CSR**, the host issues an approved constrained X.509 client certificate, and the client reconnects to a separate **mTLS-authenticated signaling** route. Native Rust/iOS/Android TLS identity interoperability must be proved by #77; unreviewed signed-challenge fallback is not allowed. A client-provided public key string alone is never proof. An SDP fingerprint is bound to the authenticated signaling session, and the DTLS identity is checked via the WebRTC library before sending frames. `connection_id` is fresh for each PeerConnection and never reused across reconnections.

### Catalog/capabilities

```text
NotSubscribed -> SnapshotApplied(revision=N) -> ApplyingDelta(base=N,new=N+1)
   -> SnapshotApplied(N+1)
  gap, duplicate conflict, backend restart -> NeedsResync -> full new snapshot
```

A delta with an unexpected `base_revision`, a revision rollback, invalid target reference, or malformed entry causes resync, never partial apply. Keep subscription revisions separate for catalog/capabilities/permissions and associate snapshot with connection/host generation. Unknown optional capabilities may be displayed as unavailable generic descriptors but must never trigger native actions.

### Capture and stream

```text
ListedOrPicker -> RequestedWindowSource -> NativeChooserPending
   -> ApprovedActualWindow + GrantEpoch -> StreamStarting -> FirstDecodedFrame
   -> Live -> (Reconfigure | Paused | Revoked | TargetClosed | Disconnected)
```

If the portal returns a monitor when a window was requested, return `ERROR_SCOPE_MISMATCH`, close the source and **do not stream**. No catalog ID is authoritative until binding evidence. A picker-only source appears in the UI as that approved source, not as the catalog window the client hoped to open. Permission revocation stops media at the source and invalidates generation immediately. Closing a managed application is a different operation from closing a shared-desktop window.

### Input control and replay

```text
ViewOnly -> InputGrantPending -> GrantApproved -> LeaseAcquired
   -> Active -> (LeaseRenewed | Revoked | LeaseExpired | ConnectionClosed)
                    \-> ReleaseAllHeldInputs -> ViewOnly
```

Input edges are reliable ordered, validated for sequence/lease/grant and deduplicated; motion may be lossy but cannot advance edge state. Release-all bypasses normal rate limiting and is always attempted on teardown. Duplicate destructive actions are answered with cached request result, **not re-executed**; after reconnect the client queries state instead of replaying an action. Absolute pointer/touch coordinates require matching geometry epoch. Implement finite/range checks for floats and normalised coordinates; NaN/Infinity are invalid. `CommittedText` is not a fake sequence of physical keys.

## Request/reply rules

- Every request uses an unpredictable 16-byte `request_id`; results echo it. Notifications use an empty request ID and are distinguished by a defined event payload.
- Every message checks negotiated protocol version and fresh `connection_id`. The server rejects a zero-length or invalid ID, unknown required feature, excessive array/string, mixed response/request body, invalid enum zero and malformed oneof.
- Validate on server **immediately before side effects**, not solely at snapshot publication: principal, permission, grant epoch, target generation, control lease, capability revision, action parameter schema, rate limit.
- Scope `action_id` to registered typed namespace and schema revision; unknown action returns `ERROR_UNSUPPORTED_OPERATION`; never expose arbitrary shell/exec/path-based launch parameters.
- Action request deduplication: **proposed** bounded cache keyed by authenticated principal + connection + request ID, last 1,024 results or 60 seconds (whichever first), with request-body digest mismatch rejected. On uncertain reconnect, reconcile capability/state instead of automatic resend.
- Error includes machine-stable `ErrorCode`, localizable `message_key`, correlation ID and optional retry. Unknown errors render a safe generic message. No window titles/secrets/payloads in structured errors.

## Feature negotiation and compatibility

`ProtocolRange` selects the highest mutually supported (major, minor); incompatible major fails. An optional new message/field requires a named negotiated feature before it can affect behavior. A required feature unsupported by the other side blocks only the affected capability/session when possible; bootstrap fundamentals block the connection. Use separate feature IDs for `capture.picker`, `capture.verified_catalog_binding`, `input.eis`, `input.touch`, `clipboard.text`, `managed_session`, `media.h264`, `media.audio`, and any future motion-channel optimization. Version values are not a proof of implementation; capability snapshots contain observed support.

**Compatibility matrix to test:** v1.0 peer vs v1.0; v1.0 vs v1.1 additive optional; old client sees unknown enum safely; changed field tag/type rejected in CI; revoked grant across reconnect; unknown oneof payload ignored only if optional feature; unsupported required feature rejected before side effects. Do not guarantee unknown-field round trips in JSON. Preserve binary unknowns in a relay when supported by each generated runtime; otherwise relay never parses/re-emits unrecognized messages.

## Golden-vector test contract

`golden-v1.json` contains **hand-derived, candidate** protobuf hex vectors for simple messages to anchor primitive encodings. These are not a substitute for generated-code checks and should be independently verified with protoc. Worker must add:
1. A generator that constructs deterministic messages in Rust, Swift and Android, serializes to the same byte sequence (where field order is canonicalized), and parses each language's binary output in the other two.
2. Negative fixtures: truncated varints, incorrect wire type, >limit payload, invalid UTF-8, unknown field, unknown enum, missing required-semantic value, enum zero and unexpected oneof.
3. Extended fixtures: all permission/lease/capture states, catalog revision gap, duplicate action and stale target/epoch; avoid embedding real credentials.
4. Binary golden artifact checked in with a generator commit and CI recipe; decoding round trips are mandatory, bitwise equal serialization is asserted only for explicitly deterministic fixtures.
5. Schema linter + breaking change gate; regenerate all client artifacts and verify no diff.

## Worker PR sequence (issue #56)

1. Schema syntax/tooling CI spike, missing-schema corrections and codegen tool pins.
2. Domain adapter + generated Rust types, typed error and state machine tests.
3. Swift iOS decoding/fixtures, unknown version and receipt behavior.
4. Android/Skip compatibility spike and tested Kotlin/Java generated model path (do not merely copy Swift sources).
5. WebRTC reliable-control integration with a synthetic host; keep legacy TCP adapters isolated.
6. Permission/capture/picker/reconnect conformance fixtures and migration flags; remove old production representations only via #64.

## Open technical decisions requiring an explicit review

- Confirm the selected ADR 0003 CSR/mTLS identity enrollment works with real iOS SecIdentity and Android Keystore/KeyManager, then approve it; no alternate challenge login without a new security ADR (#59).
- Whether input motion gets a separate unreliable channel; start reliable control/edges, benchmark before enabling motion optimization (#58/#65).
- Initial catalogue pagination versus a clearly bounded snapshot and fail-closed error.
- Clipboard maximum and feature rollout based on iOS/Android OS policies.
- Exact `proto3` vs protobuf Editions and Swift/Skip generator compatibility. Prefer the simplest cross-language supported syntax over new language features.
- Host identity versus WebRTC DTLS certificate rotation/pinning relationship (#59): pin the TLS host identity, bind verified SDP fingerprint to it per session, not a forever-static DTLS certificate.

## Sources and reference specifications

- Protobuf field presence and unknown fields: https://protobuf.dev/programming-guides/field_presence/
- Protobuf Editions / compatibility: https://protobuf.dev/programming-guides/editions/
- Protobuf JSON differences: https://protobuf.dev/programming-guides/json/
- Buf breaking checks: https://buf.build/docs/breaking/
- WebRTC transport security: https://www.rfc-editor.org/rfc/rfc8827
