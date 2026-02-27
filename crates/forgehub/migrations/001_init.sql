CREATE TABLE organizations (
    id   TEXT PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE workspaces (
    id                     TEXT PRIMARY KEY,
    org_id                 TEXT NOT NULL REFERENCES organizations(id),
    name                   TEXT NOT NULL,
    timezone               TEXT NOT NULL DEFAULT 'UTC',
    attention_budget_total INTEGER NOT NULL DEFAULT 12,
    repos_json             TEXT NOT NULL DEFAULT '[]',
    members_json           TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE decisions (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id),
    repo_id         TEXT NOT NULL,
    question        TEXT NOT NULL,
    context_json    TEXT NOT NULL,
    severity        TEXT NOT NULL,
    tags_json       TEXT NOT NULL DEFAULT '[]',
    impact_repo_ids TEXT NOT NULL DEFAULT '[]',
    assigned_to     TEXT,
    agent_goal      TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    resolved_at     TEXT,
    resolution_json TEXT
);

CREATE INDEX idx_decisions_workspace ON decisions(workspace_id, resolved_at);
CREATE INDEX idx_decisions_severity ON decisions(severity, created_at);

CREATE TABLE attention_budget_log (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    decision_id  TEXT NOT NULL,
    reviewer     TEXT NOT NULL,
    action       TEXT NOT NULL,
    logged_at    TEXT NOT NULL
);

CREATE TABLE replay_events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    repo_id    TEXT,
    timestamp  TEXT NOT NULL,
    elapsed    TEXT NOT NULL,
    event_type TEXT NOT NULL,
    action     TEXT NOT NULL,
    result     TEXT,
    payload    TEXT
);

CREATE INDEX idx_replay_session ON replay_events(session_id, timestamp);

CREATE TABLE session_cache (
    session_id    TEXT PRIMARY KEY,
    workspace_id  TEXT NOT NULL,
    edge_id       TEXT NOT NULL,
    hub_meta_json TEXT NOT NULL,
    state         TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
