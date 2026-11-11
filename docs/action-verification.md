# Semantic action verification

This procedure is an **opt-in manual Niri check**. It is not run by CI and must never be triggered automatically by `live-client`.

## Preconditions

- Run the host and client on the same trusted Linux desktop, using loopback only.
- Choose a disposable, visible Niri window and record its ID.
- Keep the pairing secret private. Do not paste it into logs, issue comments, or shared terminals.
- The reversible action used here is `window.focus`: it only focuses the selected window. To reverse the focus change, focus the previously active window again.

## Procedure

1. Start the host's confirmed live service with window-scoped action permission enabled. Keep the listener bound to `127.0.0.1` and use the pairing secret printed by the host only in the local client invocation.
2. Start the reference client for that same window with `--action window.focus --allow-action`. The explicit flag is mandatory; without it, the client must refuse to invoke the action.
3. Confirm that the client reports the correlated action result as `completed`. Confirm visually that Niri focused the selected window.
4. Focus the previously active window to restore the original desktop focus.
5. Close both processes and verify that the host reports one completed action, not merely one received action result.

Example client invocation (replace the endpoint, secret and window ID with the values from the local host session):

```sh
vista-host live-client \
  --connect 127.0.0.1:PORT \
  --secret-hex LOCAL_PAIRING_SECRET \
  --target window:WINDOW_ID \
  --frames 60 \
  --action window.focus \
  --allow-action \
  --json
```

## Expected safety properties

- No action is sent unless `--allow-action` is explicitly present alongside `--action`.
- The action ID must exist in the authenticated host's latest capability snapshot and be supported, enabled and granted.
- Request context is derived from the authenticated session and host snapshot. Caller-supplied session, grant, target or capability context is not accepted.
- The host revalidates the request against its own live control lease. A client-side advertised capability is not itself an authorization grant.
- Parameters are schema-validated and bounded by the action contract. The example sends no parameters and exposes no shell/command action.
- A timeout or mismatched result is an unknown outcome. Do not retry automatically; inspect the desktop and host state first.
- Rejected, failed and outcome-unknown results are not counted as completed actions.

## Evidence boundary

A successful fake-backend socket test proves authenticated transport, dispatch, correlation and accounting against the deterministic backend. It does **not** prove Niri actually performed the action. Only the opt-in manual procedure above can provide that desktop evidence. Do not claim real-desktop action verification until it has been performed and recorded.
