# Linux desktop portability: adapters, compatibility probes, evidence

Status: **planning document**. Entries below are implementation targets, not verified support claims. See [ADR 0002](../adr/0002-host-platform-neutral-core.md).

## Capability-first support policy

`LinuxDesktopRuntime` must compose separately discovered providers. Environment variables (`XDG_CURRENT_DESKTOP`, `XDG_SESSION_TYPE`, `WAYLAND_DISPLAY`, `DISPLAY`) give diagnostic hints but **must not** authorize, grant, or imply functional support. `CatalogProvider`, `CaptureProvider`, `InputProvider`, `WindowActionProvider`, `AudioProvider`, `ClipboardProvider` and `SessionRuntime` can vary independently.

Do not assume installed `xdg-desktop-portal` = functioning desktop integration. Check D-Bus availability, relevant portal interface and version, advertised source/device/cursor masks; test live operation only after opt-in user consent. The portal may use different backends per interface.

**Published states per capability:** `available`, `needs_user_consent`, `temporarily_unavailable`, `unsupported`, `error`; separately publish `granted`, `denied`, `revoked` permission state. The server revalidates before side effects. Do not silently select monitor capture after window-capture refusal.

## Initial validation matrix

| Environment | Catalog discovery | Window selection/capture | Input injection | Semantic actions | Independent app |
|---|---|---|---|---|---|
| Niri Wayland | typed niri-ipc candidate | ScreenCast portal/PipeWire; verify window picker | RemoteDesktop/EIS if supported; prove scope | limited typed Niri actions | separately managed compositor |
| GNOME Wayland | optional native adapter or picker-only | GNOME portal backend; verify selected window | test RemoteDesktop/EIS and scope | provider-specific; none by default | same managed session runtime |
| KDE Plasma Wayland | optional native adapter or picker-only | KDE portal backend; verify selected window | test RemoteDesktop/EIS and scope | provider-specific; none by default | same managed session runtime |
| wlroots-based desktop (choose sway/hyprland) | optional compositor adapter or picker-only | installed portal backend; verify WINDOW capability | verify RemoteDesktop/EIS availability, not assumed | provider-specific; none by default | same managed session runtime |
| X11 desktop | **future evaluation only** | do not assume Wayland/portal path applies | different native provider likely | different native provider likely | future |

**No row is verified just because the software claims a feature.** Scope minimum viable experience: an explicitly selected *single window* streamed to iOS with truthful control availability. A desktop with working capture but no input can still offer a clearly labelled view-only session. Desktop-wide capture is a separate explicit mode and consent.

## Exact probe output to design

Each JSON run includes:
- `host_os`, `display_server`, `desktop_hint`, compositor version if available, `portal_frontend_version`, selected backend interface versions;
- `capabilities`: source types WINDOW/MONITOR/VIRTUAL, cursor modes, RemoteDesktop device types, EIS availability, clipboard, catalog, action providers, audio routing, private-session support;
- `operations`: `probe_only` by default; optional `consent_window`, `consent_input`, `verify_action`, `teardown`;
- `result`: structured typed state/error, correlation ID and duration, **no titles, thumbnails, pixels, clipboard payload, private paths, secrets or unredacted window text in logs**.

Distinct test phases: (1) non-invasive discovery; (2) interactive, explicit grant; (3) authorized bounded capture/input; (4) permission revocation, window closure, compositor restart; (5) cleanup and reconnect. No automated smoke test may open a chooser or inject keys without an explicit opt-in switch.

## Scenarios and fixtures

1. `catalog_present_without_capture`: enumerate Niri windows but refuse a false capture claim.
2. `picker_only`: no compositor IPC; choose a window through the portal and stream it without pretending to know its compositor window ID.
3. `capture_yes_input_no`: show view-only stream, disable input action, and never attempt uinput fallback.
4. `monitor_only`: WINDOW missing; refuse a window stream and offer separate monitor flow only with new consent.
5. `revoked_after_start`: stop media/input/clipboard independently and drop all held key/button state.
6. `backend_restart`: re-probe capabilities, advance generations and reject stale target/grant handles.
7. `remote_desktop_scope_is_shared`: do not label keyboard/pointer as private-window input.
8. `private_session_without_portal`: allow managed-compositor capture and session-local input through explicitly scoped adapters; do not fall back to active desktop capture.
9. `permission_persistence_unavailable`: re-request consent rather than fabricating a remembered grant.
10. `different_portal_backends`: demonstrate partial functionality and diagnostic source attribution.

## Interface-specific notes

- ScreenCast source types and cursor modes are advertised by the portal; newer versions expose `pipewire-serial` to avoid reusable node IDs. Window catalog IDs and portal stream identifiers must not be treated as equal.
- RemoteDesktop recommends EIS/libei where available; D-Bus `Notify*` methods may be a fallback per support, but must not be mixed on an established EIS connection.
- Clipboard portal requires a compatible RemoteDesktop/InputCapture session and `RequestClipboard` *before* `Start`. Its grant is separate from streaming access. MIME data transfer is size-bounded and revoked atomically.
- Private compositor capture/input path is **not** assumed to expose a login-session portal. Select per-session native interfaces only after explicit process/socket ownership checks. Private sockets do not automatically sandbox the launched app.
- Audio sources may be system output, application-specific, or unavailable. Record verified scope; never claim per-application isolation merely because a PipeWire monitor node exists.

## Evidence / definition of support

For every tested environment record: distribution, session type, compositor version, portal frontend/backend(s) and versions, PipeWire, GPU/driver, selected grants, actual provider method, release of permissions/resources, measured stream stability and known limitations. Keep a dated, reproducible result attachment or markdown table and link to GitHub issue(s). Do not maintain a single yes/no `supports Vista` switch.

## Authoritative platform docs

- XDG ScreenCast: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html
- XDG RemoteDesktop: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html
- XDG Clipboard: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Clipboard.html
- Portal backend selection: https://flatpak.github.io/xdg-desktop-portal/docs/writing-a-new-backend.html
- Niri screencasting: https://niri-wm.github.io/niri/Screencasting.html
