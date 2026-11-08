# Vista architecture package

Status: design draft, 2026-11-08. Goal: [#8](https://github.com/goncalompontes/vista/issues/8).
These documents specify intended behavior; they do not implement a protocol or establish runtime support.

## Confirmed product decisions

| ID | Decision | Consequence |
| --- | --- | --- |
| D1 | Closely adapt Sill's interface and portable interactions | Inventory before porting; preserve provenance and notices |
| D2 | LAN and Tailscale/direct connections first | No account, hosted rendezvous, public relay or mandatory cloud dependency in the initial design |
| D3 | Existing-window control may affect local focus/pointer after explicit approval | Clearly label Shared desktop; separate input requires an independent session |
| D4 | Remember viewing access; approve control for each session | Pairing establishes identity; it never bypasses portal consent or grants permanent control |
| D5 | Last-session tile images are opt-in | Metadata tiles by default; local image retention and deletion controls when enabled |

The maintainer selected all five decisions in this session. Other technical choices below are proposals awaiting implementation review, not user-approved performance or security guarantees.

## Reading order and ownership

| Document | Owns |
| --- | --- |
| [Sill inventory](sill-inventory.md) | Source evidence, feature inventory, adaptation and parity gaps |
| [Protocol](protocol.md) | Messages, capability/action contract, portable input and bounds |
| [Lifecycle and security](lifecycle-security.md) | Trust, consent, session states, reconnect and revocation |
| [Client experience](client-experience.md) | Screens, responsive arrangements, gestures and visible states |
| [Host and media](host-media.md) | Adapter boundaries, isolated-session model, transport options and media lifecycle |
| [Scenarios and delivery](scenarios.md) | Cross-document acceptance scenarios, deferred decisions and work sequence |
| [Fixtures](fixtures.json) | Illustrative contract/scenario data; not a complete JSON Schema or test implementation |

An implementation MUST follow explicit permission/target checks in this package. Capitalized requirements describe the proposed contract, not existing code.

## Current repository boundary

At main e49e1e5: Niri discovery/catalog/portal capability probes; SwiftUI and browser previews with fictional data. Unmerged PR #6 at 777bf7a adds bounded capture/codec/session/recovery diagnostics; actual portal-approved Niri capture remains unverified. Draft PR #1 at 1764b65 adds Linux iOS tooling; it records successful tool builds, but no completed iOS VM boot or Vista IPA execution.

This branch starts from main and does not merge or overwrite either feature branch. Do not treat the proposed protocol session model as an API already implemented by host/src/session.rs on PR #6: that struct currently protects diagnostic frame queues, not paired remote sessions.

## Implementation policy

Use fake hosts/producers to build the client and contract first; validate native execution, capture, control scope, and media performance on hardware before promoting capabilities. Keep #3 (security/protocol), #4 (client), #5 (independent sessions), #7 (capabilities) linked. This package alone closes none of those implementation issues.

Use stable semantic IDs and backend adapters. Do not fork UI behavior on OS names; presentation is driven by capabilities, targets and permissions. Native iOS behavior and accessibility remain the client's responsibility.
