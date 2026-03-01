import test from "node:test";
import assert from "node:assert/strict";
import { formatAge, sortDecisions } from "../components/decision_utils.js";

test("formatAge returns minutes for small values", () => {
  const now = new Date("2025-01-01T00:10:00Z");
  const created = new Date("2025-01-01T00:07:30Z");
  assert.equal(formatAge(created.toISOString(), now), "2m");
});

test("sortDecisions orders by severity then timestamp", () => {
  const list = [
    { id: "1", severity: "low", created_at: "2025-01-01T00:00:10Z" },
    { id: "2", severity: "critical", created_at: "2025-01-01T00:01:00Z" },
    { id: "3", severity: "critical", created_at: "2025-01-01T00:00:01Z" },
  ];
  const sorted = sortDecisions(list);
  assert.deepEqual(sorted.map((d) => d.id), ["3", "2", "1"]);
});
