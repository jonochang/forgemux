import test from "node:test";
import assert from "node:assert/strict";
import { contextColor, riskColor, severityColor, statusColor } from "../components/shared.js";
import { T } from "../theme.js";

test("contextColor matches risk thresholds", () => {
  assert.equal(contextColor(69), T.ok);
  assert.equal(contextColor(70), T.warn);
  assert.equal(contextColor(85), T.warn);
  assert.equal(contextColor(86), T.err);
});

test("riskColor maps levels", () => {
  assert.equal(riskColor("green"), T.ok);
  assert.equal(riskColor("yellow"), T.warn);
  assert.equal(riskColor("red"), T.err);
});

test("severityColor maps severity", () => {
  assert.equal(severityColor("critical"), T.err);
  assert.equal(severityColor("high"), T.molten);
  assert.equal(severityColor("medium"), T.warn);
  assert.equal(severityColor("low"), T.info);
});

test("statusColor maps states", () => {
  assert.equal(statusColor("unreachable"), T.t4);
  assert.equal(statusColor("errored"), T.err);
  assert.equal(statusColor("waiting"), T.warn);
  assert.equal(statusColor("idle"), T.info);
  assert.equal(statusColor("running"), T.ok);
});
