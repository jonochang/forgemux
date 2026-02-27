import test from 'node:test';
import assert from 'node:assert/strict';
import { severityRank, decisionAgeMinutes, sortDecisions, filterByRepo } from './decision_utils.js';

test('severityRank orders by priority', () => {
  assert.equal(severityRank('critical'), 0);
  assert.equal(severityRank('high'), 1);
  assert.equal(severityRank('medium'), 2);
  assert.equal(severityRank('low'), 3);
  assert.equal(severityRank('unknown'), 4);
});

test('decisionAgeMinutes uses now override', () => {
  const now = new Date('2026-02-27T00:10:00Z');
  const created = '2026-02-27T00:05:00Z';
  assert.equal(decisionAgeMinutes(created, now), 5);
});

test('sortDecisions sorts by severity then age', () => {
  const list = [
    { id: '1', severity: 'low', created_at: '2026-02-27T00:10:00Z' },
    { id: '2', severity: 'high', created_at: '2026-02-27T00:05:00Z' },
    { id: '3', severity: 'high', created_at: '2026-02-27T00:01:00Z' },
  ];
  const sorted = sortDecisions(list);
  assert.deepEqual(sorted.map((d) => d.id), ['3', '2', '1']);
});

test('filterByRepo filters by repo_id', () => {
  const list = [
    { id: '1', repo_id: 'a' },
    { id: '2', repo_id: 'b' },
  ];
  assert.equal(filterByRepo(list, 'a').length, 1);
  assert.equal(filterByRepo(list, 'all').length, 2);
});
