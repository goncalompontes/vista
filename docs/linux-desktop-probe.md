# Linux desktop capability probe — issue #69

Status: **passive diagnostic implemented; desktop interoperability remains unverified**. This is not proof of functional capture, permission, input scope, or independently managed sessions.

## Safe commands

```sh
cargo run -p vista-host -- desktop-probe --json
cargo run -p vista-host -- desktop-probe-watch --json
```

The default and watch commands issue **only** session D-Bus `GetId`, `ListNames`, `GetNameOwner`, `Properties.Get`, and `Introspect` (with no auto-activation), plus a Niri IPC `Version` request if `NIRI_SOCKET` exists. All D-Bus calls have two-second timeouts. No chooser, screencast session, pixel read, clipboard access, input injection, persistent permission request, window enumeration, or window-title logging occurs. The watch command re-probes every three seconds; it advances `capability_revision` on observation changes and `generation` on service-owner/bus/compositor epoch changes. Revisions are local to the running process.

Do not confuse `supported: advertised` (the frontend returned a relevant bit or interface version) with a verified operation. `permission: not_requested`, `verified: false`, and `operation: probe_only` are intentional. `available` means only unprivileged metadata adapter reachability, never a grant. `needs_user_consent` is a request precondition, not a guarantee of success. `unsupported` and `temporarily_unavailable` distinguish negative published bitmasks from missing evidence. `scope: shared_desktop` is explicitly **not** window-scoped input. The scoped-input capability remains unsupported and the probe never substitutes uinput. A catalog is not correlated with portal-selected pixels. `portal_picker_only` means only that WINDOW source selection is advertised and an application *could ask* the portal; no native window ID association is claimed.

`running_backends` merely enumerates owned `org.freedesktop.impl.portal.desktop.*` bus names and reported properties. It does **not** assert which backend the portal frontend routed to: `selected_for` remains empty. Distribution, desktop and session values are sanitized hints only, not authorization. Package/frontend backend versions and PipeWire runtime versions are explicitly marked unobserved rather than fabricated. Audio is not queried; no sources are claimed. Environment secrets and D-Bus error bodies are omitted.

## Hardware-free scenarios

Run `cargo test -p vista-host --offline desktop_probe`. The input fixture `docs/fixtures/linux-desktop-probe.json` exercises these **synthetic** cases:

| Desktop class | Scenario | Expected result |
|---|---|---|
| Niri | WINDOW advertised, no verified catalog | portal-picker-only |
| GNOME Wayland | WINDOW and RemoteDesktop advertised | picker-only; no grants |
| KDE Plasma Wayland | WINDOW without RemoteDesktop | view-only picker |
| Sway / wlroots | MONITOR only | WINDOW unsupported, no monitor fallback |
| Niri | portal unavailable | unknown capability |
| GNOME | MONITOR + virtual monitor, no WINDOW | WINDOW unsupported |
| KDE | RemoteDesktop returns no devices | keyboard unsupported |
| Sway | incomplete properties | unknown, fail closed |
| Niri | catalog and WINDOW advertised | catalog remains unbound |
| GNOME | RemoteDesktop without ScreenCast | no capture |
| KDE | multiple running backend candidates | no routing attribution |
| Sway | future unknown source/device bits | no implied WINDOW/keyboard |

The tests check no fixture grants permission, verifies capture, or advertises scoped input. Owner-change tests check epoch generation changes; passive re-probing never transfers permission. These fixtures model advertised frontend properties, **not** successful desktop usage.

## Explicitly opt-in manual validation (four desktops)

Record the exact distro/kernel/session, desktop/compositor version, `xdg-desktop-portal` and backend package versions, PipeWire version, chosen backend configuration, diagnostic JSON with timestamp, and failures. Remove hostnames, usernames, window titles, stream IDs, secrets, screenshots, clipboard contents, and IPs from shared logs.

1. **Niri Wayland:** compare Niri's officially configured portal provider with the diagnostic. If a window chooser is desired, explicitly run existing `capture-consent --confirm` and record the chosen **window** scope and whether selection succeeded. Only then opt in to `capture-monitor --confirm` for bounded transient frame counting (max 3 seconds/120 frames); it saves no pixels. Confirm session cleanup after cancellation/normal exit.
2. **GNOME Wayland:** repeat the passive report and separately opt in to WINDOW consent/frame verification. Record that availability is independent of Niri catalog/actions.
3. **KDE Plasma Wayland:** verify composite backend inventory, WINDOW mask, RemoteDesktop device mask, and clipboard version independently. Do not assert a backend route from name presence.
4. **Sway wlroots:** first determine if WINDOW is genuinely advertised. When absent, record view-only unsupported — never substitute a monitor automatically. Evaluate xdg-desktop-portal-wlr ScreenCast/Screenshot limitations, and optional maintained generic backend as a separately configured experiment, not a global replacement.

There is **no safe scoped-input validation command in this change**. Do not use `--allow-input`, `uinput`, a privileged virtual-input device, or an unverified global fallback as an EIS test. A future consent-bound input adapter must establish permission scope, independently verify target ownership, enforce lease revocation, and cleanup. Clipboard tests require their own explicitly authorized session and must not log data. Independent-session work is tracked under #62.

Compare distro-packaged GNOME/KDE/niri portal configurations before trialing xdg-desktop-portal-generic; xdg-desktop-portal-wlr's documented scope is ScreenCast/Screenshot, not universal RemoteDesktop. Evaluate generic backend maturity/license, real WINDOW support, EIS semantics, latency/copies and clipboard as an experimental spike, not assumed compatibility.

## Scope and limits

The diagnostic is observational: a frontend can advertise a WINDOW source while refusing the chooser, and distinct portal interfaces may route to distinct backend services. The frontend does not supply authoritative routing provenance through the probed properties. Re-probe after bus/backend/compositor restarts; an existing capture/input lease requires separate invalidation in its owning provider. Existing `capture-consent`, `capture-frame`, and `capture-monitor` are separate explicit-consent tools, not invoked by the diagnostic. No complete hosted permission lifecycle or network protocol capability publication is introduced here. Follow-up work must wire diagnostic states into the portable authenticated session capability publisher and implement scoped grants/revocation.
