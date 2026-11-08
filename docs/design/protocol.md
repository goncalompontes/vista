# Proposed Vista protocol v1

Status: architecture draft; no listener or interoperable implementation exists. Related: #3, #7.
Wire names here are proposed, not compatibility promises. JSON fixtures are illustrative; full schema generation is implementation work.

## Connections, discovery and versions

Direct endpoints over LAN/Tailscale are address hints, never identity. Bonjour/mDNS advertises only service version, instance ID and endpoint; no window titles, pairing secret or captured images. Manual hostnames/IPs support Tailscale where multicast discovery is unavailable. Client deduplicates verified hosts by pinned identity, not address. Never infer trust from a tailnet connection.

Proposed control plane: TLS 1.3 + WebSocket with one JSON envelope per logical message. Negotiate major/minor in hello. Reject incompatible major versions before catalogs or actions; optional unknown fields may be ignored, unknown required extensions fail explicitly. Disable application side effects via TLS early data. Pairing is a restricted bootstrap channel, not the authenticated catalog channel.

Envelope fields: v (major integer), type (registered string), id (request string or null), connection_epoch (fresh host-issued string). Post-auth requests also include session_id when scoped to a stream, and grant_epoch for input/actions. IDs are opaque, case-sensitive, nonempty strings <=128 UTF-8 bytes. Client invents no host IDs.

Each connection starts with hello/hello.result (supported versions and client decoding/input limits), authentication, then fresh catalog/capability/permission snapshots. Bootstrap pairing has its own challenge context and cannot use post-auth messages. Host issues connection_epoch after authentication; hello itself omits it. Reconnection creates a new epoch.

## Catalog and targets

A catalog entry is a window, launchable application, explicit desktop source, or managed session. It carries an opaque target_id, target_generation, display metadata, source kind and capability references. App launch creates a target/session; it does not reuse an arbitrary executable path supplied by the client. A full snapshot has catalog_revision; deltas require the exact base revision or trigger resync. A restarted compositor creates fresh generations even when numeric Niri IDs repeat.

No catalog before authentication and view authorization. Capturable does not mean capture already consented. Names and thumbnails can be sensitive.

## Capabilities and actions

Capabilities attach to a host/session/window/application target. Descriptor:

| Field | Meaning |
| --- | --- |
| id / schema_version | Namespaced semantic ID and supported parameter version |
| target_id / target_generation | Exact object and lifetime |
| supported | Backend can implement the defined semantics |
| enabled / reason | Currently meaningful; e.g. no focused window |
| authorization | granted, required, denied or revoked; never inferred from supported |
| parameters | Reference to a bounded registered schema, not executable validation code |
| presentation | Bounded label, known icon hint and optional shortcut display |
| suggested_bindings | Recognized client gesture names; suggestions subject to user/accessibility policy |
| capability_revision | Snapshot revision for this target |

Unknown optional actions can appear in a generic actions list only if the client understands their declarative parameter schema; otherwise omit them. Standard client controls require recognized semantic IDs. Host text is untrusted plain text; no HTML, script, external icon downloads or automatic URL navigation.

Core semantic IDs: desktop.launcher.open; desktop.workspace.step(direction=previous|next); desktop.overview.show; desktop.show_desktop; window.focus; window.maximize.set(value=boolean); window.minimize; window.close; window.fit_viewport(width,height); application.launch; edit.copy; edit.paste; menu.fetch; menu.invoke. Advertising a semantic ID commits to its stated effect, not just a vaguely related key chord. Use namespaced extensions for non-equivalent compositor concepts.

All host-global desktop actions require a distinct desktop.actions grant even when a selected window has input permission. Window-only capture consent is not a desktop-control grant. Clipboard transfers are separate messages/permissions.

Example action request:

```json
{"v":1,"type":"action.invoke","id":"r17","connection_epoch":"c1","session_id":"s1","grant_epoch":"g2","target_id":"w5","target_generation":"niri-boot1-w5","capability_revision":12,"action":"window.maximize.set","schema_version":1,"parameters":{"value":true}}
```

Result references id and reports status=completed|rejected|failed|outcome_unknown, error_code where applicable, and authoritative resulting state/revisions when observable. Accepted/queued is not completed. Reject stale epochs/targets/revisions with stale_context and request a refresh. Adapters must report unsupported/unobservable outcomes honestly.

Errors include unsupported_action, invalid_parameters, permission_required, permission_denied, stale_context, target_gone, busy, rate_limited, backend_unavailable and outcome_unknown. A failed connection after close/launch/paste does not imply the action failed: never automatically replay side effects.

Deduplicate action request IDs within the connection: retain the last 1024 completed results for 60 seconds; if the bound is exhausted inside the window, reject new action requests as busy rather than evicting protected entries. Reject conflicting reuse of an ID. After expiry the client MUST use a new ID and MUST NOT retry a possibly executed side effect. Never promise exactly-once execution across host crashes.

## Portable input

Input is authorized per target and ordered input stream, using session_id, connection_epoch, grant_epoch, input_seq and geometry_epoch. Proposed v1 uses reliable ordered control delivery with a separate bounded input queue and prioritized cleanup. Latest pointer motion may be coalesced only before serialization and never across a button/key boundary. Backpressure beyond a small time budget cancels control rather than delivering a long stale key queue.

| Event | Fields / semantics |
| --- | --- |
| pointer.absolute | x,y in [0,1] of the active target content rectangle; reject outside content |
| pointer.relative | dx,dy in negotiated source-pixel units; advertise only where available |
| pointer.button | stable primary/secondary/middle/back/forward, down/up; ordered |
| scroll | dx,dy, unit=pixel|line, phase=begin|update|end|cancel, momentum boolean, anchored pointer location |
| key.physical | USB HID keyboard page usage, down/up, repeat flag; left/right modifiers are physical keys |
| text.commit | UTF-8 committed text; bounded; not a key chord |
| input.cancel | Clear all held buttons/keys, scroll and composing state for this control lease |

Do not send committed text and synthesized letter keys for the same typing operation. Client-side IME composition stays local until committed; a future composition protocol must be negotiated separately. Key meanings follow the host layout. Editing toolbar actions use semantic edit.* rather than rewriting physical Ctrl/Cmd keys. Hardware keyboard remapping, if offered, is an explicit client setting; raw physical mode remains available.

Absolute coordinates are obtained by reversing client picture zoom/pan, orientation, letterboxing and content-rect transforms. Host validates geometry_epoch before injection. On resize, suspend absolute input until the client has a matching rendered geometry, cancel active drag/keys, and resume explicitly. Do not silently clamp a click onto an unintended edge.

Client handles natural-scroll preference once; host adapter converts protocol units to its native API without applying another user inversion. Pixel scroll is optional; line-only backends declare the limitation. Native acceleration/pointer confinement and precision require hardware validation.

## Gesture bindings and menus

Client gesture recognizer emits a local intent; binding resolution chooses local operation or semantic action. Precedence: OS/accessibility reservations > explicit user binding > compatible host suggestion > client default. Host suggestions cannot replace reserved navigation, pairing or permission controls. Provide buttons/accessibility actions for every optional multi-finger action. A cancelled gesture sends no action and cannot leak pointer/click events.

Menu descriptors use opaque IDs, labels, enabled/checked state, bounded children and menu_revision. Fetch lazily. Invocation includes application target generation and menu_revision; stale menu invocation is rejected, never resolved by label or index. A menu grant is not arbitrary accessibility-tree access.

## Proposed limits

These are conservative starting bounds, negotiable downward; measure before increasing.

| Resource | Initial ceiling |
| --- | --- |
| Control message after decompression | 256 KiB; WebSocket compression disabled initially |
| Catalog | 256 entries/page, 4096/session; explicit truncation/pagination |
| Actions | 256/target; 64 parameters/action |
| Label / reason | 256 / 1024 UTF-8 bytes |
| Menu | depth 6, 512 items/fetch, 4096 cached/session |
| Text commit | 16 KiB; larger text uses explicit clipboard transfer |
| Clipboard text | 1 MiB, one transfer at a time per direction |
| Input | 240 events/s sustained, burst 480; cleanup must not be throttled |
| Action requests | 20/s, launch/close <=2/s |
| Outstanding requests | 32/connection; bounded response deadlines |
| Session scope | One active video target/client in initial release |

Control rejection occurs before allocation/injection. Reject non-finite coordinates, unknown enums, duplicate object keys and invalid UTF-8. Streaming parsers enforce size/depth bounds before constructing full object trees.

Permission and state semantics live in [lifecycle-security](lifecycle-security.md). [Fixtures](fixtures.json) specify example outcomes; they are not a replacement for normative schemas.
