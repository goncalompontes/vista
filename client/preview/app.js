// Browser-only Vista UX preview. This is intentionally not a SwiftUI runtime.
// Keep this catalog and screen sequence aligned with client/core's DemoCatalog
// and client/ios/Vista/Views until a real host API replaces the demo.
const apps = [
  { id: "demo-browser", name: "Browser", subtitle: "Web workspace", symbol: "◎", source: "desktop", sourceLabel: "Existing window" },
  { id: "demo-terminal", name: "Terminal", subtitle: "Shell session", symbol: "⌘", source: "desktop", sourceLabel: "Existing window" },
  { id: "demo-editor", name: "Code Editor", subtitle: "Development", symbol: "⌘", source: "desktop", sourceLabel: "Existing window" },
  { id: "demo-music", name: "Music Player", subtitle: "Media", symbol: "♫", source: "independent", sourceLabel: "Independent app" }
];

export const initialState = () => ({ screen: "welcome", filter: "all", selected: null, device: "iphone" });
export function reduce(state, action) {
  switch (action.type) {
    case "RESET": return { ...initialState(), device: state.device };
    case "DEVICE": return ["iphone", "ipad"].includes(action.device) ? { ...state, device: action.device } : state;
    case "SCREEN": return ["welcome", "library", "stream"].includes(action.screen)
      ? { ...state, screen: action.screen, selected: action.screen === "stream" ? (state.selected || apps[0].id) : state.selected } : state;
    case "FILTER": return ["all", "desktop", "independent"].includes(action.filter) ? { ...state, filter: action.filter } : state;
    case "OPEN": return apps.some(a => a.id === action.id) ? { ...state, screen: "stream", selected: action.id } : state;
    default: return state;
  }
}

const root = typeof document !== "undefined" ? document.getElementById("app-root") : null;
const shell = root ? document.getElementById("device-shell") : null;
const stateLabel = root ? document.getElementById("debug-screen") : null;
const filterLabel = root ? document.getElementById("debug-filter") : null;
const appLabel = root ? document.getElementById("debug-app") : null;
let state = initialState();
const button = (text, action, className = "") => `<button type="button" class="${className}" data-action="${action}">${text}</button>`;

function welcome() {
  return `<div class="page welcome">
    <div class="logo" aria-label="Vista">▣</div>
    <h2>Your Linux apps,<br><span class="accent">anywhere.</span></h2>
    <p>A native home for your Linux desktop apps on iPhone and iPad.</p>
    <div class="feature-panel">
      <div class="feature-row"><span class="symbol">▣</span> Your open windows</div>
      <div class="feature-row"><span class="symbol">▤</span> Independent app sessions</div>
      <div class="feature-row"><span class="symbol">☝</span> Native touch and keyboard</div>
    </div>
    ${button("Explore the app preview  →", "library", "primary")}
    <p class="small-print">Preview only · No host connection or streaming yet</p>
  </div>`;
}
function library() {
  const visible = apps.filter(a => state.filter === "all" || a.source === state.filter);
  return `<div class="page">
    <div class="topbar">${button("‹  Exit preview", "welcome", "back")}</div>
    <div class="heading"><div class="logo small">▣</div><div><h2>Vista</h2><p>Your apps</p></div><span class="pill">DEMO</span></div>
    <div class="notice"><span class="symbol">⌁</span><div><strong>No Linux host paired</strong><p>This is a local interface preview. The catalog below is sample data, not discovered windows.</p></div></div>
    <h3>Applications</h3>
    <div class="filterbar" role="group" aria-label="Filter applications">
      ${[["all","All apps"],["desktop","On desktop"],["independent","Independent"]].map(([id,title]) => `<button type="button" class="${state.filter===id?"active":""}" aria-pressed="${state.filter===id}" data-filter="${id}">${title}</button>`).join("")}
    </div>
    <div class="app-grid">${visible.map(app => `<button type="button" class="tile" data-open="${app.id}" aria-label="Open ${app.name} preview">
      <span class="tile-icon" aria-hidden="true">${app.symbol}</span>
      <span class="tile-info"><strong>${app.name}</strong><span class="desc">${app.subtitle}</span><div class="kind">${app.sourceLabel}</div></span>
    </button>`).join("")}</div>
    <p class="footnote">Streaming, host pairing and input are not available in this preview.</p>
  </div>`;
}
function stream() {
  const app = apps.find(a => a.id === state.selected) || apps[0];
  return `<div class="page">
    <div class="topbar">${button("‹  Applications", "library", "back")}</div>
    <div class="streambox"><span class="big-symbol">${app.symbol}</span><span class="muted-icon">⊘</span>
      <strong>No active stream</strong>
      <p>Live video will appear here once a Linux host, capture pipeline and secure transport are connected.</p>
    </div>
    <div class="stream-title"><h2>${app.name}</h2><span>${app.sourceLabel}</span>
    <p>This screen previews Vista's app session layout. No pixels, audio, clipboard data or input are transmitted.</p></div>
    <div class="controls"><span>⌖ Pointer</span><span>⌨ Keyboard</span><span>♫ Audio</span></div>
  </div>`;
}
function render() {
  if (!root) return;
  root.innerHTML = state.screen === "welcome" ? welcome() : state.screen === "library" ? library() : stream();
  shell.classList.toggle("iphone", state.device === "iphone");
  shell.classList.toggle("ipad", state.device === "ipad");
  stateLabel.textContent = state.screen;
  filterLabel.textContent = state.filter;
  appLabel.textContent = state.selected || "none";
  document.querySelectorAll("[data-device]").forEach(el => {
    el.classList.toggle("active", el.dataset.device === state.device);
    el.setAttribute("aria-pressed", String(el.dataset.device === state.device));
  });
  document.querySelectorAll("[data-go]").forEach(el => el.classList.toggle("active", el.dataset.go === state.screen));
}
function dispatch(action) {
  const next = reduce(state, action);
  if (next === state) return;
  state = next;
  if (root) root.scrollTop = 0;
  render();
  console.debug("[Vista preview]", action, state);
}
if (root) {
  document.addEventListener("click", event => {
    const target = event.target.closest("button");
    if (!target) return;
    if (target.dataset.device) dispatch({ type:"DEVICE", device:target.dataset.device });
    else if (target.dataset.go) dispatch({ type:"SCREEN", screen:target.dataset.go });
    else if (target.dataset.action) dispatch({ type:"SCREEN", screen:target.dataset.action });
    else if (target.dataset.filter) dispatch({ type:"FILTER", filter:target.dataset.filter });
    else if (target.dataset.open) dispatch({ type:"OPEN", id:target.dataset.open });
    else if (target.id === "reset-preview") dispatch({ type:"RESET" });
  });
  render();
}
