CREATE TABLE handoffs (
    id                  TEXT PRIMARY KEY,
    role_from           TEXT NOT NULL,
    role_to             TEXT NOT NULL,
    status              TEXT NOT NULL,
    session_id_from     TEXT,
    artifact_type       TEXT NOT NULL,
    summary             TEXT NOT NULL,
    acceptance_json     TEXT NOT NULL DEFAULT '[]',
    github_owner        TEXT NOT NULL,
    github_repo         TEXT NOT NULL,
    github_issue_number INTEGER NOT NULL,
    github_pr_number    INTEGER,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    claimed_by          TEXT,
    completed_by        TEXT
);

CREATE INDEX idx_handoffs_queue ON handoffs(role_to, status, updated_at DESC);
CREATE INDEX idx_handoffs_github ON handoffs(github_owner, github_repo, github_issue_number);
