# Vista browser UI preview (Arch Linux)

This is an **interactive HTML/CSS/JS mirror of the early Vista SwiftUI app
shell**. It is **not an iPhone emulator**, does not execute SwiftUI, and cannot
debug the iOS SDK, Metal, video decoding, audio, app lifecycle, signing, or
device integration. Keep the browser mirror and SwiftUI screens aligned
manually. It runs without npm installation or external network dependencies.

## Run

From the repository root:

```sh
cd client/preview
python3 -m http.server 4173 --bind 127.0.0.1
```

Open **http://127.0.0.1:4173/** in Firefox. The preview starts on the
welcome screen. Explore the app library, switch the catalog filter, open a
sample app and return. Use the built-in iPhone/iPad size toggle for a quick
layout check.

For debugging in Firefox:

- **F12** opens DevTools (Inspector, Debugger, Console and Network).
- **Ctrl+Shift+M** toggles Responsive Design Mode; choose a phone-sized
  viewport and touch emulation.
- Edit `index.html`, `styles.css` or `app.js`, then refresh the tab.
- The preview logs state transitions under `[Vista preview]` in the console.
- Run `npm test --prefix client/preview` from the repository root to test
  the navigation state machine and the capability demo (Node 22+ recommended).
- The stream screen's **Host actions** panel is driven by
  `capabilities.js`. Use the inspector's **Capability backend** toggle to show
  the same renderer against a Niri capability set and a simulated alternate
  backend. Unsupported, disabled and denied actions stay visibly unavailable;
  allowed actions update the capability revision and visible state. This is a
  local contract demonstration, not native action execution.

The server binds only to loopback. Do not change it to `0.0.0.0` on a
public network without understanding the exposure.

## Native iOS testing

The **real** app is in `../ios/` (SwiftUI, XcodeGen). An Apple-compatible
macOS Xcode environment is required to compile or run its iOS Simulator.
Use a Mac, remotely accessible Mac, or macOS CI. The browser mirror is for
fast UI iteration and cannot validate native behavior. See `../README.md`.

## DSH plugin research

DSH plugins are installed **per profile** (e.g. `dsh-tui`), not globally.
The current dsh-tui profile already contains terminal, TUI, goal,
ChatGPT-web bridge and PTC plugins.

Useful community candidates to evaluate for browser preview regression:

- [dsh-pilot](https://github.com/Viger1/dsh-pilot): Playwright browser
  operation, accessibility references and an explicit permission model.
- [dsh-browser](https://github.com/ben7am1n/dsh-browser): lightweight
  Playwright browser navigation, clicks, screenshots.
- [dsh-plugin-browser](https://github.com/xu1132/dsh-plugin-browser):
  headless Playwright screenshot and interaction tools.

These **do not emulate iOS**. They usually require Chromium and a browser
runtime, neither of which is installed by default on this machine. Inspect
their source, compatibility and permissions before adding any third-party
plugin. Installing into the currently running DSH profile generally requires
restarting it, which would interrupt active sessions. No DSH plugins are
installed as part of this preview setup.

Typical install after review and browser setup:

```sh
dsh plugin --profile dsh-tui add dsh-pilot
```
