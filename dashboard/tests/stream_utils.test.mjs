import test from "node:test";
import assert from "node:assert/strict";
import { updateLastSeen } from "../lib/stream.js";

test("updateLastSeen ignores unknown payloads", () => {
  assert.equal(updateLastSeen(4, null), 4);
  assert.equal(updateLastSeen(4, { type: "ack" }), 4);
  assert.equal(updateLastSeen(4, { type: "event", event_id: "nope" }), 4);
});

test("updateLastSeen advances on event id", () => {
  assert.equal(updateLastSeen(0, { type: "event", event_id: 3 }), 3);
  assert.equal(updateLastSeen(5, { type: "event", event_id: 4 }), 5);
});

test("updateLastSeen advances on snapshot id", () => {
  assert.equal(updateLastSeen(2, { type: "snapshot", snapshot_id: 7 }), 7);
  assert.equal(updateLastSeen(9, { type: "snapshot", snapshot_id: 6 }), 9);
});
