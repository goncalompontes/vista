import test from "node:test";
import assert from "node:assert/strict";
import { initialState, reduce } from "./app.js";

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
