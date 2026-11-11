# Portal source identity and picker-only capture

Status: implemented host-side identity model slice for issue #71. This contract is deliberately conservative: the current ashpd capture path receives a portal-selected stream and PipeWire node, but does not expose a trustworthy mapping to a Niri catalog entry. It therefore cannot create a `VerifiedTargetBinding`.

## Identity types and scope

- `CatalogTargetRef`: a compositor/application catalog object plus its target generation.
- `PortalSelectedSourceRef`: the source actually selected by the user in the portal, including portal session, source type and PipeWire node/serial. The optional `verified_identity` is reserved for a future provider-attested opaque token; never derive it from a title, PID, app ID, geometry, focus or row order.
- `CaptureGrantRef`: the portal grant and its source/session.
- `VerifiedTargetBinding`: exists only when provider-attested identity is present and all scope/session checks pass.
- `StreamTarget::PickerOnly`: an approved source that has no catalog correlation. It is a separate target variant with its own lifecycle generation, not a catalog target with a guessed ID.

All references are scoped by host, backend, host session, backend generation and permission epoch. A mismatch rejects the operation. A new portal session or permission epoch must not reuse a previous binding.

## Picker sequence

```mermaid
sequenceDiagram
    participant C as Client
    participant H as Host
    participant P as ScreenCast portal
    C->>H: request WINDOW capture (optional catalog hint)
    H->>P: create session; select_sources(WINDOW only)
    P-->>H: user approves or cancels
    alt cancelled
        H-->>C: chooser_cancelled; no stream target
    else approved
        P-->>H: selected source + PipeWire stream
        H->>H: validate scope, portal session, grant and source type
        alt provider identity proves catalog correspondence
            H->>H: create VerifiedTargetBinding
            H-->>C: catalog target stream
        else no verified correlation
            H->>H: create PickerOnly target + lifecycle generation
            H-->>C: picker-only stream (no catalog identity)
        end
    end
    P-->>H: revocation / stream loss
    H->>H: revoke grant and retire target generation
    H-->>C: capture_revoked or source_lost
```

## Required behavior

1. A WINDOW request selects only WINDOW sources. A monitor/virtual/unknown result is rejected; it is never silently substituted.
2. Portal cancellation creates no grant and no stream target. Partial negotiation must close the portal session and publish no active target.
3. The PipeWire node ID is a transport locator, not source identity. Node reuse and `pipewire-serial` changes do not create or preserve catalog bindings.
4. Without a provider-attested identity token, stream as picker-only. The UI must label it as portal-selected and must not show catalog-target identity, title, or controls as if correlation were proven.
5. On portal restart, revocation, source closure or stream loss, revoke the grant and retire the picker-only lifecycle generation. Re-selection creates a new portal session and generation.
6. Input scope is independent from video source identity. A single-window video stream may still have explicitly granted shared-desktop input; that permission is not implied by the capture grant.

## Wire/UI shape

The Rust model uses a tagged `StreamTarget` union. The client should render catalog-bound and picker-only targets as separate rows/sections. Picker-only is a stream destination, not a selectable catalog item and not a persistent catalog entry. The host must not serialize a synthetic catalog ID for it.

## Test coverage in this slice

Unit tests cover missing correlation, provider-attested correlation, stale host generation and permission epoch, portal session mismatch, node/serial reuse, and refusal of monitor/virtual/unknown sources for a WINDOW request. Portal cancel/revocation and actual provider correlation require integration coverage when the live portal adapter exposes those lifecycle events and a stable identity token.
