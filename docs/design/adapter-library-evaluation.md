# Existing Linux host libraries and protocol reuse: evidence-based selection

Status: **research decision record, 2026-11-09**. No new dependency installed, licensed, compiled or runtime-verified. Inputs: Vista `host/Cargo.toml` at the read date (Rust 1.88; `ashpd 0.13.13`, `gstreamer 0.24`), #68–#72, WebRTC #58 and proposed ADR #67. Current versions below are **observations as of review, not automatic upgrade instructions**.

## Engineering rule

Use maintained standards/libraries for security, media, signaling infrastructure, portal access, native compositor IPC and process supervision. Vista owns only its distinctive domain semantics: host identity/policy, target+grant+generation references, capability/action semantics, input leases, session lifecycle and client UX. Do not create custom Wayland extensions, audio framing, RTP/ICE/DTLS, or portal substitutes before showing a measured gap in existing maintained facilities.

Every candidate must be evaluated on: actual capabilities (not README promises), upstream activity and release cadence, Rust/MSRV/Swift/Android constraints, supported compositor versions, stable API, permissive/GPL license compatibility, binary/package complexity, security review exposure, latency/zero-copy, accessibility/consent, teardown and reliable test doubles.

## Component-by-component recommendation matrix

| Domain | Reuse candidates | Proposed choice / status | Why / caveat |
| --- | --- | --- | --- |
| XDG portal client APIs | **ashpd 0.13.13**; higher-level **lamco-portal 0.4.5** | Keep existing ashpd initially; spike lamco-portal only if it removes material lifecycle code | Vista already uses ashpd; avoid two independent portal clients and duplicate consent paths. lamco-portal wraps ashpd and adds convenience, but its policy semantics need audit. |
| Niri listing/semantic actions | **Niri public JSON-over-Unix-socket protocol**; official `niri-ipc 26.4.0` for reference | **Preferred: independently authored async JSON socket adapter** if Apache-2.0 is approved (#73) | Niri documents one line-delimited JSON request/reply per socket and a separate long-lived event-stream socket. Upstream crate inherits GPL-3.0-or-later and changes Rust API with Niri versions. Reuse the public protocol without copying GPL implementation; no per-action CLI subprocess. |
| GNOME/KDE catalog actions | Native compositor/DE APIs, portal chooser | **No mandatory native catalogue initially**; picker-only via portal | An all-desktops window enumeration API is not provided by ScreenCast. Design provider optional; do not invent GNOME shell extension solely for Vista M1. |
| Shared Wayland WINDOW capture | XDG ScreenCast + PipeWire via ashpd / GStreamer | **Production default, consented source only** | User picker selects actual source; do not derive PipeWire source from native window ID or silently broaden to monitor. Niri docs explicitly use xdg-desktop-portal-gnome. |
| Portal backend: Niri | **xdg-desktop-portal-gnome** (niri official recommendation) | **Baseline to validate first** | Niri documentation explicitly requires working D-Bus session, PipeWire and this portal for window/monitor screencasting. Do not replace working setup with generic backend by assumption. |
| Portal backend: GNOME/KDE | xdg-desktop-portal-gnome / -kde | **Native backend default** | Desktop-specific consent/input/clipboard support varies by version; user portal configuration determines actual backend per interface. |
| Portal backend: wlroots | xdg-desktop-portal-wlr; **xdg-desktop-portal-generic 1.2.0** | Probe separately; test generic as a promising candidate, **not** immediate universal replacement | xdp-wlr documents ScreenCast + Screenshot only. Generic claims ScreenCast v6, RemoteDesktop v2, Clipboard v1 with ext-/wlr- fallbacks, but new release and runtime support/capture scope need independent proof. |
| Remote input | Portal RemoteDesktop v2 + libei/EIS; Rust **reis 0.7.1**; D-Bus Notify | Prefer portal+EIS if verified; direct Notify fallback only when permitted | reis upstream documents incomplete/unstable API; isolate it behind InputProvider and pin tested revision. After ConnectToEIS, don't also use Notify methods. No hidden `uinput` privilege escalation. |
| Clipboard | XDG Clipboard v1 with RemoteDesktop/InputCapture session; ashpd feature | **Reuse ashpd** + new grant/lifecycle wrapper | Must call RequestClipboard before session Start; support varies across KDE/GNOME. Separate auth, bounded MIME text initially; no arbitrary file transfer. |
| Media ingest | Existing GStreamer 0.24 `pipewiresrc`; pipewire-rs 0.10.1 if low-level needed | **Keep GStreamer** for pipeline; use pipewire-rs only to solve a real metadata/buffer-control gap | No general CPU-RGBA requirement; prototype currently copies in diagnostics, but production should benchmark DMA-BUF/zero-copy. |
| Encoding/video | GStreamer H.264 encoder elements; WebRTC RTP | **Keep existing GStreamer**; runtime probe actual encoder initialization | NVENC/VAAPI/software vary by GPU and plugin/license. No Vista-defined H.264 record protocol in production. |
| WebRTC host | GStreamer **webrtcbin** vs **rswebrtc/webrtcsink** | Small comparative spike before binding production code | webrtcsink wraps webrtcbin and ships default signaling useful for fixed media, but Vista needs authenticated custom signaling, data channels, target/grant policy. Choose webrtcbin if customization/permission semantics are simpler; don't rewrite ICE/SDP/media congestion. |
| Private managed compositor | **Weston pipewire backend**, Weston nested/headless, labwc, sway | **Start research with Weston pipewire backend**; not yet selected | Weston docs explicitly list pipewire output backend. Need prove app surface selection, input injection, GPU acceleration, audio/clipboard and teardown; pipewire output alone is not full independent app streaming. |
| X11/Xorg future | `x11rb`, EWMH/ICCCM, XComposite/XDamage, XTest | Research #72; not Wayland-first release gate | Capability and security model differs; explicit X11 shared-desktop risks and per-window capture/occlusion limitations. |
| LAN host discovery | DNS-SD/mDNS maintained Rust crate, iOS Network.framework | Standard DNS-SD; no custom broadcast protocol | Address metadata is not trust; user manual/Tailscale fallback mandatory. Pin only authenticated host identity. |
| Protocol generation | buf, prost, swift-protobuf, Java/Kotlin protobuf | Binary protobuf with generated models | Select pinned generators and verify Skip compatibility rather than assuming Swift source sharing implies wire sharing. |
| Transport security | TLS 1.3 + WebRTC DTLS-SRTP | Standard libraries only | Authenticated signaling pins host; bind received SDP DTLS fingerprint to signaling principal. No custom records or key exchange. |

**Governance review:** [ADR 0004](../adr/0004-license-and-niri-ipc-policy.md) recommends Apache-2.0 and Niri public JSON IPC rather than linking the GPL helper, pending owner approval #73. Any earlier typed-crate recommendation in this file is superseded by this candidate.

## New findings materially affecting the architecture

### 1. Niri has a documented portable ScreenCast path

Niri's screencasting guide currently recommends PipeWire + `xdg-desktop-portal-gnome` and reports both monitor and individual window screencasting, provided niri is started as a proper session with D-Bus. That is strong documentary support for making **portal capture independent of Niri IPC**. Niri IPC is useful for catalog and actions, not a precondition to media.

Sources: https://niri-wm.github.io/niri/Screencasting.html ; https://niri-wm.github.io/niri/Important-Software.html

### 2. Reusing niri-ipc creates a licensing decision

The crate's docs currently show release **26.4.0**, direct request/reply plus event stream sockets, and explicit non-stable Rust API. Its Cargo metadata inherits **GPL-3.0-or-later** from niri. Vista currently has no root LICENSE file visible in the inspected repository tree; selecting a project license must precede publishing binaries that link GPL code. Don't casually import the crate into a supposedly permissive core. A Linux-specific optional dynamic adapter does **not** automatically make a proprietary/permissive binary exempt; obtain actual license review. Alternatives: accept a compatible license, communicate via publicly documented JSON protocol through independently written adapter, or genuinely separate distribution with clear obligations. Avoid copying GPL code when choosing JSON.

Source: https://docs.rs/crate/niri-ipc/latest/source/Cargo.toml.orig ; https://docs.rs/niri-ipc/latest/niri_ipc/

### 3. The generic portal backend deserves a spike, but not a blanket switch

As of review, `xdg-desktop-portal-generic 1.2.0` was released **2026-11-07** and advertises ScreenCast v6, RemoteDesktop v2, Clipboard v1 via standard ext/wlr protocols. This may reduce compositor-specific hacks and be useful on controlled nested sessions. However, its README and crate existence do **not** demonstrate that all compositors support actual WINDOW-scoped source picking or portal consent the same way. It is a recently evolving integration; compare against Niri's officially documented GNOME portal and GNOME/KDE native backends. Evaluate additional build deps, runtime D-Bus needs, EIS fallbacks, safe clipboard and hardware copies.

Source: https://docs.rs/crate/xdg-desktop-portal-generic/latest ; https://github.com/lamco-admin/xdg-desktop-portal-generic

### 4. Weston provides a concrete PipeWire-backed private-compositor option

The current Weston docs list a `pipewire` backend that runs without local input and outputs to a PipeWire node, alongside `headless`, `wayland`, `x11`, `drm` and `rdp` backends. This makes Weston a stronger early *capture-output* candidate than "launch arbitrary compositor and hope a portal chooser works". It still may composite an entire managed output rather than one app window and requires separate evidence for scoped virtual input, app ownership, multi-window handling and sandbox claims.

Source: https://wayland.pages.freedesktop.org/weston/toc/running-weston.html

### 5. Higher-level portal wrappers and WebRTC sinks are genuine wheel-reuse candidates

`lamco-portal 0.4.5` released 2026-08-29, MIT/Apache-2.0, wraps ashpd for capture/input/clipboard. Compare actual code eliminated against added indirection; changing from working ashpd should be earned. GStreamer's `webrtcsink` wraps `webrtcbin` and offers default/custom signallers; review whether Vista's custom authenticated pairing/data-channel/lease lifecycle can be integrated cleanly before implementing a bespoke signaling wrapper around raw webrtcbin.

Sources: https://docs.rs/crate/lamco-portal/latest ; https://gstreamer.freedesktop.org/documentation/rswebrtc/index.html

## Comparison and feasibility experiments (research-only setup now)

| Spike | Cases to measure on actual Linux runtime | Accept / reject criteria |
| --- | --- | --- |
| A. Portal backend selection | Niri official GNOME portal, GNOME Wayland, KDE Wayland, wlroots configured portal, candidate generic backend | Working WINDOW source + readable PipeWire frame after user consent; correct source kind; denial/teardown, EIS input and clipboard states independently |
| B. Typed Niri IPC | Niri matching version, mismatched version, two sockets, restart, event missed | Generated catalogue matches snapshot, no blocking, truthful actions, license gate passed |
| C. Input implementation | EIS via reis, optional D-Bus Notify where allowed | Pointer/key reach granted seat; no stuck keys on cancel; actual scope reported; no unauthorized uinput fallback |
| D. WebRTC library | webrtcbin vs webrtcsink using synthetic H.264 + authenticated signaling fixture | Data-channel access/identity binding, reconnection/IDR, low-latency settings, package size and minimal custom infrastructure |
| E. Private compositor | Weston pipewire, Weston headless, labwc, sway candidate | Owned socket/process; media node/surface capture; scoped input; hardware or explicit software fallback; no host-desktop pixels; clean crash teardown |
| F. Media zero-copy | GStreamer PipeWire/DMABUF vs current diagnostic RGBA path | Same image quality, frame pacing, GPU/CPU utilization and measured copies; choose measured better path |
| G. X11 future | x11rb/XComposite/XDamage/XTest on one Xorg desktop | Explicit source scope and input risks; no false Wayland/portal assumptions |

Proposed evidence JSON for each run:
```json
{
  "environment": "name/version/session-type",
  "provider": "name/version/backend-interface",
  "capability": "window_capture|input|clipboard|audio|managed_session",
  "probed": true,
  "user_grant_requested": false,
  "actual_verified": false,
  "source_scope": "window|monitor|session|shared_desktop|unknown",
  "permission_scope": "string or unknown",
  "cleanup_verified": false,
  "latency_distribution_ms": null,
  "source_commit": "exact pinned revision",
  "limitations": ["no assumed runtime support from docs"]
}
```

No user content, window title, pixels, copied text or secrets in benchmark JSON. A manually consented probe is separate from detection, has a bounded duration, and never silently enables global input.

## Recommended adoption order

1. **Retain:** existing ashpd, GStreamer and portal/pipewire architecture. Add capability checks and host provider seam (#68/#69).
2. **Investigate:** upstream Niri IPC + its GPL implications before merging #57. Niri is optional; picker-only host works without it.
3. **Evaluate:** generic portal backend on test configuration, not user's main GNOME/KDE/Niri portal blindly.
4. **Spike:** webrtcbin vs webrtcsink after #56 schema, including exact data-channel/authenticated signaling requirements.
5. **Prove:** Weston pipewire capture as one managed-session option before requiring a headless portal in #62/#70.
6. **Defer:** X11 native APIs until after portable core works; no Windows/macOS adaptation code now.

## Worker deliverables and no-code work that can close design subissues

- Library inventory with exact dependency versions/links, release history, license/SBOM, MSRV, device support, maintained alternatives and migration risk.
- Decision records for portal wrapper and WebRTC library; "choose upstream" alone is insufficient without API sketch and measurement plan.
- Compatibility matrix (Niri, GNOME, KDE, wlroots, managed Weston) with verified vs hypothesized status; no fictional green rows.
- Scripts/checklists for opt-in consent probes, synthetic media, clipboard and input cleanup; actual runtime results captured later.
- Adapter error translation maps and mock test fixtures; no need to install graphical stack to write them.

## Sources

- Niri IPC: https://docs.rs/niri-ipc/latest/niri_ipc/
- Niri screen capture: https://niri-wm.github.io/niri/Screencasting.html
- ashpd: https://docs.rs/ashpd/latest/ashpd/desktop/screencast/index.html
- Portal RemoteDesktop and Clipboard: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html ; https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Clipboard.html
- Portal generic: https://docs.rs/crate/xdg-desktop-portal-generic/latest
- Portal wlr: https://github.com/emersion/xdg-desktop-portal-wlr
- lamco-portal: https://docs.rs/crate/lamco-portal/latest
- reis: https://docs.rs/reis/latest/reis/
- Weston backend docs: https://wayland.pages.freedesktop.org/weston/toc/running-weston.html
- GStreamer WebRTC: https://gstreamer.freedesktop.org/documentation/rswebrtc/index.html
- PipeWire Rust: https://pipewire.pages.freedesktop.org/pipewire-rs/pipewire/
