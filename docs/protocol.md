# Vista control protocol v1

Status: **implemented in memory, not deployed.** The Rust host has a versioned
control-frame codec, a PSK-based pairing handshake, per-session key derivation,
explicit permissions and host/client state machines with negative tests. There
is **no socket, TLS/QUIC, discovery, WebRTC, media encryption, iOS client
implementation or audit** yet. This document separates the implemented contract
from the planned transport.

Related: [architecture](architecture.md), [roadmap](roadmap.md), and the
[Swift-compatible fixtures](fixtures/protocol-v1.json).

## Scope

The control protocol carries discovery results, pairing, authorization, session
lifecycle, permission changes and media negotiation. Encoded H.264 access units
are a separate media channel; this protocol does not carry pixels. The M1
`StreamSession`/`H264SyncGate` boundary remains the producer-side source of
discontinuities and keyframe requirements.

## Versioning

- The current version is **1.0**; the minimum supported version is **1.0**.
- A client sends a `min`/`max` version range. The host selects the highest
  version in the intersection with its own supported range.
- No intersection is a hard failure (`UnsupportedVersion`) before any pairing
  material is accepted.
- Additive JSON fields are ignored by version-1 decoders. Removing or changing
  field semantics requires a major-version bump.
- There is no downgrade without an intersection; a version-2-only client cannot
  fall back to version 1 silently.

## Framing

```text
+----------------------+--------------------------------------+
| u32 big-endian length| UTF-8 JSON message (length bytes)    |
+----------------------+--------------------------------------+
```

- One frame contains exactly one [`Envelope`](../host/src/protocol.rs).
- The JSON body is capped at **64 KiB**; larger frames are rejected before
  parsing. Zero-length frames are malformed.
- The control connection must be reliable and ordered. Each direction has its
  own `seq`, starting at `0` and increasing by exactly one. A duplicate, gap or
  replayed frame fails the handshake closed.
- Media must **not** share this framing or these sequence numbers. A separate
  media transport carries access units and has its own loss/recovery rules.

Envelope shape:

```json
{
  "seq": 0,
  "type": "client_hello",
  "versions": { "min": { "major": 1, "minor": 0 }, "max": { "major": 1, "minor": 0 } },
  "client_id": "iphone-15",
  "client_nonce": "000102...1e1f"
}
```

## Pairing

Pairing uses a **256-bit secret transferred out of band**, for example in a QR
code shown by the host and scanned by the client, or copied through a trusted
local channel. It is never sent over the control connection.

A six-digit code is deliberately not accepted. The confirmation tags would be
offline-guessable by a passive eavesdropper, and no PAKE is implemented. A
future short-code flow needs SPAKE2/OPAQUE (or Noise with a PSK and SAS
verification), not a truncated HMAC.

### Sequence

```text
client                                      host
  |  ClientHello { versions, client_id,       |
  |                client_nonce }  (seq 0)    |
  |------------------------------------------>|
  |                                           | validates version, id, nonce
  |  ServerHello { selected, host_id,         |
  |                host_name, host_nonce,     |
  |                pairing_id, expires_at,    |
  |                policy }  (seq 0)          |
  |<------------------------------------------|
  |                                           |
  |  PairingRequest { pairing_id, tag }       |
  |  (seq 1)                                  |
  |------------------------------------------>|
  |                                           | checks expiry + pairing_id,
  |                                           | verifies client tag
  |  PairingAccepted { tag, session_id,       |
  |                    permissions } (seq 1)  |
  |<------------------------------------------|
  |                                           |
  |  SessionOpen { session_id, permissions }  |
  |  (seq 2)                                  |
  |------------------------------------------>|
  |  SessionOpened { session_id, permissions }|
  |  (seq 2)                                  |
  |<------------------------------------------|
```

`expires_at` is the host's Unix time when the offer stops being valid. The host
enforces it; clients should also stop presenting an expired offer. The default
TTL is 120 seconds.

### Transcript and keys

Both sides build the same canonical transcript:

```text
vista-pairing-v1
version=<major>.<minor>
client_id=<client_id>
client_nonce=<client_nonce>
host_id=<host_id>
host_nonce=<host_nonce>
pairing_id=<pairing_id>
policy=<bits>
```

(Each line ends with `\n`; identifiers are printable ASCII and cannot contain a
newline. `policy` is a bit mask: bit 0 video, bit 1 audio, bit 2 input, bit 3
clipboard.)

Confirmation tags are HMAC-SHA256:

```text
client_tag = HMAC-SHA256(secret, "client-confirm\n" || transcript)
host_tag   = HMAC-SHA256(secret, "host-confirm\n"   || transcript || "\nsession=" || session_id)
```

The host verifies the client tag in constant time before accepting pairing; the
client verifies the host tag before deriving keys. The session identifier is
generated by the host and returned in `PairingAccepted`; it must be treated as
opaque and validated as 16 bytes.

Session keys are HKDF-SHA256 over the same secret:

```text
salt = pairing_id
ikm  = secret
info = "vista-session-v1\n" || pairing_id || "\n" || session_id || "\n" || transcript
```

64 bytes are expanded and split into independent **client-to-host** and
**host-to-client** keys.

### Encrypted record layer (loopback diagnostic)

After `PairingAccepted`, both sides switch from plaintext control frames to
ChaCha20-Poly1305 records:

```text
+----------------------+--------------------------------------+
| u32 big-endian length| ciphertext incl. 16-byte tag         |
+----------------------+--------------------------------------+
```

- Plaintext is the same JSON `Envelope`; the maximum plaintext is 64 KiB.
- The nonce is an implicit 12-byte value: four zero bytes followed by a
  big-endian 64-bit per-direction counter starting at 0. Both sides increment
  after every record; replayed or reordered records fail authentication.
- The AAD is `"vista-record-v1" || session_id || direction`, where direction is
  `1` for client-to-host and `2` for host-to-client. This binds a record to its
  session and prevents reflection.
- The client encrypts with `client_to_host` and decrypts with `host_to_client`;
  the host does the opposite.
- Keys are unique per handshake because `pairing_id` and both nonces are fresh.

`vista-host protocol-loopback --json` binds `127.0.0.1:0`, performs the full
pairing over a real TCP stream, switches to encrypted records and verifies an
encrypted `SessionOpen`, `KeyframeRequest`, `Ping`/`Pong` and `SessionClose`.
It is a local diagnostic only. It is **not** TLS/QUIC/WebRTC, does not hide
metadata or packet sizes, has no congestion control, reconnect logic or
discovery, and must not be exposed to an untrusted network in this form.

For a two-process check, `vista-host protocol-serve --confirm [--listen ADDR]`
starts a one-shot control listener. It prints a JSON offer with the bound
address, pairing id, expiry and the out-of-band secret, accepts exactly one
client, runs the same pairing and encrypted session, prints the server stats
and exits. It binds `127.0.0.1` by default and refuses a non-loopback address
unless `--allow-remote` is passed explicitly. `vista-host protocol-client
--connect ADDR --secret-hex HEX` is a reference client that performs the
pairing and an encrypted `KeyframeRequest`/`Ping` exchange. Neither command
captures or streams media; the secret printed by the server is the
QR-equivalent pairing material and must be treated as sensitive.

## Sessions and permissions

Every capability defaults to denied:

```json
{ "video": true, "audio": false, "input": false, "clipboard": false }
```

- The host advertises its policy in `ServerHello` and `PairingAccepted`.
- The client requests a subset in `SessionOpen`. The host grants the requested
  subset or rejects the whole request; it never silently adds a capability.
- Input and clipboard have no implicit grant. A host must obtain explicit user
  consent before advertising them in its policy.
- `audio` is reserved for a later media/audio implementation and is denied by
  default.
- Revocation is a host-side action that closes the session; a transport must
  also stop delivering media and drop the session keys.

State machines:

```text
host:   AwaitingHello -> AwaitingPairing -> Paired -> Active -> Closed
                                        \-> Failed on expiry/auth/permission/sequence errors

client: AwaitingHello -> AwaitingServerHello -> AwaitingPairingAccepted
        -> Paired -> AwaitingSessionOpened -> Active -> Closed / Failed
```

The host is authoritative. A failed authentication, expired offer, sequence
error or permission escalation is terminal for that connection. A new
connection requires a new pairing exchange.

`KeyframeRequest` asks the producer for a fresh IDR (the media layer maps it to
an encoder force-key-unit request when supported). `Ping`/`Pong` are liveness
checks. `SessionClose` is a clean shutdown; transport loss must also revoke the
session.

## Media boundary

After `SessionOpened`, media is expected to be H.264 Annex-B access units, one
IDR/recovery point per keyframe, and the `H264SyncGate` rules from the capture
path apply:

- New consumers receive a discontinuity before data and wait for an
  SPS/PPS-prefixed IDR.
- Queue overflow, geometry change, producer `DISCONT` or a reconnect must be
  surfaced as a discontinuity; delta units are suppressed until a fresh IDR.
- `KeyframeRequest` should be forwarded to the producer. The current encoder
  diagnostic does not yet implement active IDR requests.

This document does **not** define packetization, congestion control, RTP
depacketization, jitter buffering or iOS VideoToolbox integration.

## Transport choice: WebRTC vs custom

Requirements: mutual authentication, encryption, NAT traversal, congestion
control, loss recovery, keyframe requests, iOS support and bounded buffering.

| Concern | WebRTC | Custom TLS/QUIC + RTP |
| --- | --- | --- |
| Encryption | DTLS-SRTP, mature | TLS 1.3/QUIC, mature |
| Congestion control | GCC/transport-cc, NACK, PLI/FIR | must be built or borrowed |
| NAT traversal | ICE/STUN/TURN | must be built |
| iOS integration | WebRTC.framework, VideoToolbox-friendly | lower-level, more client work |
| Implementation size | large dependency, standard interop | smaller surface, more media work |

**Recommendation:** use WebRTC for the media path and carry discovery, pairing,
authorization and session control over an authenticated TLS/QUIC stream running
this protocol. Let WebRTC provide DTLS-SRTP, ICE, congestion control and
NACK/PLI/FIR rather than inventing them. A custom transport is only justified if
WebRTC's dependency and signaling complexity become a demonstrated problem.

This is a design recommendation, not an implemented transport.

## Threat model and trust boundaries

Protect:

- captured pixels and encoded media;
- input events and clipboard contents;
- the pairing secret and derived session keys;
- the user's ability to see and revoke an active session.

Adversaries:

- a passive attacker on the local network;
- an active man-in-the-middle who can relay or modify control frames;
- a malicious or compromised client;
- a stolen client device or leaked QR code;
- a client that sends oversized, replayed or out-of-order frames;
- a client that requests capabilities it was not granted.

Mitigations implemented here:

- a 256-bit out-of-band secret, HMAC-SHA256 mutual confirmation and HKDF session
  keys;
- a versioned, size-bounded JSON frame codec with strict per-direction
  sequence numbers;
- short-lived pairing offers, opaque pairing/session identifiers and fail-closed
  authentication;
- deny-by-default video/audio/input/clipboard permissions and host-side grant
  checks;
- bounds on identifiers, dimensions and frame rate in `VideoConfig`;
- a diagnostic that exercises wrong-secret, replay, expiry, escalation and
  version-mismatch paths.

Residual risks and non-goals:

- **No forward secrecy.** Compromise of the long-lived pairing secret compromises
  sessions derived from it. Rotate/re-pair after suspected compromise.
- **No host identity signature or attestation.** The QR/secret is the only root
  of trust; a user who scans an attacker's QR pairs with the attacker.
- **No transport yet.** Control frames are plaintext JSON until the planned
  TLS/QUIC record layer exists; the derived keys are not used for encryption.
- **No cross-connection replay window.** Sequence numbers protect one ordered
  connection; a transport must provide its own replay protection.
- **No rate limiting, DoS hardening, audit or external review.**
- **No discovery.** mDNS/TXT is a future addition; discovery output is untrusted
  until pairing verifies the secret.
- Metadata (host/client identifiers, timing) is visible to the transport and
  may be visible on the local network before encryption is added.

## Swift integration contract

The JSON is intentionally `Codable`-friendly:

- `type` is a snake_case string tag.
- Binary values (`client_nonce`, `host_nonce`, `pairing_id`, `session_id`,
  `tag`) are lowercase hex strings with fixed byte lengths.
- `seq` is `UInt64`; `expires_at` is Unix seconds as `UInt64`.
- `PermissionSet` fields are booleans and default to `false` when absent.
- Frames are length-prefixed JSON; the Swift client must enforce the 64 KiB
  bound and the sequence rules.
- Error codes map to a local `ProtocolError` enum; do not continue a connection
  after an authentication, expiry, sequence or permission failure.

See [`docs/fixtures/protocol-v1.json`](fixtures/protocol-v1.json) for the exact
field names and a complete synthetic exchange. Rust and Swift tests both decode
these frames. [`docs/fixtures/pairing-v1.json`](fixtures/pairing-v1.json) pins a
byte-exact transcript, confirmation tags, HKDF session keys and one media record;
both languages derive the same values.

`client/core` now implements the v1 control client in Swift: framing, the
pairing state machine, HMAC-SHA256 confirmations, HKDF-SHA256 session keys and
the ChaCha20-Poly1305 directional record layer (`ControlProtocol.swift`,
`ControlPairing.swift`, `SecureControlChannel.swift`). `swift test` covers the
fixtures, every message round-trip, expiry, wrong secret, sequence gaps and
permission escalation. The iOS target adds a Network.framework connection and a
Keychain-backed pairing sheet; it stops at an opened control session (no catalog,
video or input yet).

## Window catalog

`catalog.snapshot` carries host-owned window metadata over the authenticated
session; it is never a capability grant and never contains pixels, PIDs,
arguments or paths.

- Fields: `v`, `connection_epoch`, optional `session_id`, `generation`,
  `revision` (non-zero) and `entries`.
- An entry carries `window_id`, optional `title`, `app_id` and `workspace_id`.
- Limits: 256 entries, 256-byte titles, 128-byte app IDs, unique window IDs.
- `connection_epoch` and `session_id` must equal the authenticated session, so a
  spliced snapshot cannot cross sessions.
- `HostSession::emit_catalog` sends it through the normal host sequence.
- `protocol-serve --catalog` snapshots Niri's current window list once after
  `SessionOpened`; refresh currently means reconnecting. Live incremental
  catalog updates are future work.

Swift decodes the same fixture into `CatalogSnapshot`/`CatalogEntry`, validates
the bounds and maps entries to portable application rows. The iOS library shows
a `LIVE` badge and an honest empty state; unavailable actions remain disabled.

## Capabilities, actions and portable input

`host/src/capability.rs` implements the first transport-independent part of the
client/host contract:

- `CapabilitySnapshot` attaches versioned action descriptors to a target and
  capability revision. Each descriptor records `supported`, `enabled`, an
  authorization state, a registered parameter-schema ID, bounded presentation
  data and optional gesture hints.
- `ActionRequest`/`ActionResult` carry request IDs, connection/grant epochs,
  target generation and capability revision. The registry rejects stale
  contexts before execution, revalidates target/revision/grant/authorization,
  validates the registered parameter schema and returns `completed`, `rejected`,
  `failed` or `outcome_unknown` truthfully.
- Request IDs are de-duplicated per connection (last 1024 results, 60-second
  window); conflicting ID reuse is rejected. Action requests are limited to
  20/s and destructive close/launch requests to 2/s.
- `MenuSnapshot` bounds depth and item count and requires opaque, unique item
  IDs; invocation is expected to carry the menu revision.
- Portable input is separate from semantic actions. Physical key usage, buttons,
  relative/absolute pointer movement, scroll units/phases and committed text are
  modelled explicitly. Absolute input is rejected on a mismatched geometry
  epoch, and `InputLease::release_all` clears held keys/buttons on cancel,
  revoke or disconnect without being throttled.
- `NiriActionBackend` is the first adapter. It advertises focus/close/workspace
  step, marks maximize unsupported, and routes commands through a testable
  `CommandRunner` instead of a compositor-specific client path.

Limits: 256 actions/target, 64 parameters/action, 4 KiB parameter payload,
256-byte labels, 1024-byte reasons, menu depth 6 and 512 items, 16 KiB committed
text. Unknown parameter schemas are rejected; clients do not receive executable
validation code, shell commands or arbitrary backend access.

`vista-host actions --window ID [--allow-window-actions] [--allow-desktop-actions]
[--allow-clipboard] [--json]` now builds a live, read-only Niri snapshot for an
existing window. It reflects the requested grants: without them a supported
action is reported as `required`, never as allowed, and the command executes no
native action. The target generation is derived from the Niri socket/session
name plus the window id because Niri exposes no compositor boot id through its
CLI.

`host/src/control_lease.rs` adds a revocable per-session control lease over the
same contract. A lease owns the target, action grants and input permissions;
revocation advances the grant epoch, drops every grant and releases all held
buttons/keys, so queued actions or input from the old grant are rejected. Re-granting
also advances the epoch.

`host/src/control_dispatch.rs` binds that lease to an authenticated control
session. `HostControlChannel` derives the `ActionContext` from the lease rather
than trusting the client's request, answers `capabilities.snapshot` for the
current grant (ungranted actions appear as `required`), and routes
`action.invoke` to a truthful `action.result`. A request whose session id,
connection epoch, grant epoch, target or capability revision does not match the
lease is rejected before the backend runs; malformed wire requests produce a
structured rejection rather than a protocol failure. `HostSession::attach_control`
requires the channel to target the pairing-assigned session id, and
`HostSession::control_snapshot` emits a snapshot through the normal host
sequence. Revoke followed by re-grant leaves pre-revoke requests stale.

Fixtures shared with the Swift models are in
[`docs/fixtures/capabilities-v1.json`](fixtures/capabilities-v1.json). Rust
tests decode and validate them, and `vista-host capability-test --json` exercises
the rejection, availability, revision, duplicate, reconnect and cleanup paths.
`vista-host control-dispatch-test --json` runs a real in-memory
`HostSession`/`ClientSession` pairing, attaches a dispatcher, verifies the
snapshot and a completed action, and confirms duplicate, revoked, stale-grant,
wrong-session, wrong-target and malformed requests are rejected.

Native pointer/keyboard injection and menu retrieval from real applications are
still open. The one-shot `protocol-serve` listener remains video-only by default,
but now accepts an explicit action policy:
`--window ID` plus `--allow-window-actions`, `--allow-desktop-actions`,
`--allow-clipboard` and/or `--allow-input`. With a policy it attaches the
dispatcher after `SessionOpened`, pushes the grant-scoped snapshot, and routes
`action.invoke`; `--fake-actions` selects the deterministic backend for
diagnostics, while the default uses `NiriActionBackend` against the named window.
`protocol-client --action ID --allow-input` reads the snapshot and sends one
action, and the CLI integration tests prove the granted and ungranted paths over
the encrypted socket. Client UI wiring, native input injection and live Niri
action verification remain open.

## Implemented, verified and pending

Implemented and unit-tested in `host/src/protocol.rs`:

- version negotiation, bounded framing, strict sequencing;
- PSK pairing, HMAC confirmation, HKDF-SHA256 session keys, expiry and
  identifier/nonce validation;
- host and client state machines, permissions and denial paths;
- `vista-host protocol-test --json` in-memory diagnostic;
- Rust decoding of the Swift-compatible fixtures;
- the capability/action/portable-input contract in `host/src/capability.rs`,
  its shared fixtures and the `capability-test --json` diagnostic;
- the session-bound dispatcher in `host/src/control_dispatch.rs`, including
  `attach_control`, snapshot and action-result routing, and the
  `control-dispatch-test --json` diagnostic.

Test evidence on the development host:

```text
cargo test --workspace
  ... protocol tests: 15 passed
  ... CLI tests: 2 passed
cargo run -- protocol-test --json
  protocol 1.0, Active/Active, keys match,
  wrong secret/replay/expiry/escalation/version mismatch all rejected,
  verified: true
cargo run -- control-dispatch-test --json
  in-memory pairing attached to a control lease,
  snapshot + completed action, duplicate cached,
  revoked/stale-grant/wrong-session/wrong-target/malformed rejected,
  verified: true
```

Not implemented:

- sockets, TLS/QUIC, AEAD records, nonce management and transport replay
  protection;
- mDNS discovery and a pairing QR/user interface;
- persistent host identity, certificate pinning or attestation;
- WebRTC signaling/media, RTP packetization, congestion control and keyframe
  request wiring;
- native input injection, clipboard transfer, menu retrieval, audio and iOS
  client code;
- rate limiting, audit and external security review.

Do not describe this as a secure remote-streaming implementation until those
transport and client pieces exist and are verified end to end.

### Additive v1 stream sharing scopes (Android usability milestone)

`stream.started` may include `capture_scope` and `input_scope`. The current
`serve` producer reports `capture_scope: "display"` and
`input_scope: "shared_desktop"`, including when selecting a window: selection
fullscreens a window but capture/input still affect the display/shared seat.
These fields describe actual sharing, not the selected target's kind or name.
Older hosts omit them; missing or unrecognized values mean **unknown**, never
private-window capture/control. Clients must keep that uncertainty visible.

Both fields are optional strings for backward-compatible v1 decoding. The
shared `docs/fixtures/stream-scopes-v1.json` is exercised by Rust, Swift and
Android tests. These messages announce encoded geometry; only native decoder
render callbacks establish live playback.
