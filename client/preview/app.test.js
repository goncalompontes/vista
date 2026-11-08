import test from "node:test";
import assert from "node:assert/strict";
import { initialState, reduce } from "./app.js";
import {
  backends,
  createCapabilityState,
  invokeAction,
  renderableActions
} from "./capabilities.js";

test("screen transitions follow the SwiftUI preview flow", () => {
  const first = initialState();
  assert.equal(first.screen, "welcome");
  const library = reduce(first, {type:"SCREEN", screen:"library"});
  assert.equal(library.screen, "library");
  const stream = reduce(library, {type:"OPEN", id:"demo-browser"});
  assert.equal(stream.screen, "stream");
  assert.equal(stream.selected, "demo-browser");
});

test("filters and invalid actions are bounded", () => {
  const first = initialState();
  assert.equal(reduce(first, {type:"FILTER", filter:"independent"}).filter, "independent");
  assert.equal(reduce(first, {type:"FILTER", filter:"bogus"}), first);
  assert.equal(reduce(first, {type:"OPEN", id:"bogus"}), first);
  assert.equal(reduce(first, {type:"SCREEN", screen:"mystery"}), first);
});

test("reset clears navigation but retains the device viewport", () => {
  let state = initialState();
  state = reduce(state, {type:"DEVICE",device:"ipad"});
  state = reduce(state, {type:"OPEN",id:"demo-terminal"});
  state = reduce(state, {type:"RESET"});
  assert.deepEqual(state, {...initialState(), device:"ipad"});
});

test("both backends expose the same renderable capability shape", () => {
  const niri = renderableActions(createCapabilityState("niri"));
  const alternate = renderableActions(createCapabilityState("alternate"));
  assert.ok(niri.length > 0 && alternate.length > 0);
  for (const entry of [...niri, ...alternate]) {
    assert.deepEqual(Object.keys(entry).sort(), ["hint", "id", "invokable", "label"]);
  }
  assert.equal(backends.niri.targetId, "w5");
  assert.equal(backends.alternate.targetId, "w5");
});

test("backends differ only by declared capability state", () => {
  const niri = createCapabilityState("niri");
  const alternate = createCapabilityState("alternate");
  assert.equal(invokeAction(niri, "window.focus").status, "completed");
  assert.equal(invokeAction(alternate, "window.focus").status, "completed");
  assert.equal(invokeAction(niri, "window.maximize.set").errorCode, "unsupported_action");
  assert.equal(invokeAction(alternate, "window.maximize.set").status, "completed");
  assert.equal(invokeAction(niri, "edit.copy").errorCode, "permission_denied");
  assert.equal(invokeAction(alternate, "window.minimize").errorCode, "backend_unavailable");
  assert.equal(invokeAction(niri, "window.nonexistent").errorCode, "unsupported_action");
});

test("an allowed action updates capability state and revision", () => {
  let state = createCapabilityState("alternate");
  const result = invokeAction(state, "window.maximize.set");
  assert.equal(result.status, "completed");
  assert.equal(result.state.maximized, true);
  assert.equal(result.state.revision, 13);
  assert.equal(result.state.actions.find(a => a.id === "window.maximize.set").label, "Restore");
  state = result.state;
  const restored = invokeAction({ ...state, revision: 13 }, "window.maximize.set");
  assert.equal(restored.state.maximized, false);
  assert.equal(restored.state.revision, 14);
});

test("reducer switches backends without changing navigation or device", () => {
  let state = initialState();
  state = reduce(state, { type: "DEVICE", device: "ipad" });
  state = reduce(state, { type: "OPEN", id: "demo-browser" });
  state = reduce(state, { type: "BACKEND", backend: "alternate" });
  assert.equal(state.backend, "alternate");
  assert.equal(state.screen, "stream");
  assert.equal(state.device, "ipad");
  assert.equal(state.capabilities.label, backends.alternate.label);
  assert.equal(reduce(state, { type: "BACKEND", backend: "bogus" }), state);
});

test("reducer records rejections without pretending the action ran", () => {
  let state = initialState();
  state = reduce(state, { type: "CAPABILITY", id: "window.maximize.set" });
  assert.equal(state.capabilities.revision, 12);
  assert.equal(state.capabilities.lastResult.status, "rejected");
  assert.equal(state.capabilities.lastResult.errorCode, "unsupported_action");

  state = reduce(state, { type: "CAPABILITY", id: "window.focus" });
  assert.equal(state.capabilities.revision, 13);
  assert.equal(state.capabilities.lastResult.status, "completed");
});
