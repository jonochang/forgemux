# Epic: Role Workflow + GitHub Integration

## Objective

Implement a role-based handoff workflow in Forgemux, using GitHub Issues/PRs as the canonical work-item system.

## Source Documents

- Scenarios source of truth: `docs/specs/scenarios.md`
- Test planning source of truth: `TEST_PLAN.md`
- Design source of truth: `docs/specs/design.md`
- Product framing source of truth: `docs/specs/brief.md`

This epic plan is an execution index. It should reference and align to the docs above, not replace them.

## Outcome

- Teams can move work across roles (`product_manager -> researcher -> designer -> implementer -> reviewer_tester -> sre`) with explicit handoffs.
- Every handoff is linked to a GitHub Issue (and optional PR).
- Forgemux queues and session execution stay in sync with GitHub state.

## Scope

- In scope:
  - Role-aware handoff model and queue operations
  - GitHub issue/PR linking
  - Webhook ingestion + reconciliation sync loop
  - CLI and dashboard UX for queue/claim/complete/promote
  - BDD coverage for key user flows
- Out of scope (for this epic):
  - Non-GitHub providers (Jira, Linear)
  - Complex workflow DSL
  - Cross-org marketplace features

## Architecture Decisions

- Canonical source of truth:
  - GitHub: issue/PR lifecycle and labels
  - Forgemux: claim/queue/session runtime state
- Sync model:
  - Fast path: webhook events
  - Correctness path: periodic reconciliation by watermark
- Conflict handling:
  - Never silently overwrite; mark `needs_attention` and log audit event

## Milestones

1. Data model + API contracts
2. GitHub read integration (validate and enrich work items)
3. Queue operations (create/claim/complete/promote)
4. GitHub write-backs (comments/labels/assignees)
5. Webhook + reconciliation sync
6. Dashboard/CLI UX polish
7. BDD integration coverage + release

## Implementation Backlog

## M1: Data Model + Contracts

- [x] Add `Role` enum: `product_manager`, `researcher`, `designer`, `implementer`, `reviewer_tester`, `sre`
- [x] Add `HandoffRecord` with:
  - [x] `id`, `role_from`, `role_to`, `status`
  - [x] `session_id_from`, `artifact_type`, `summary`, `acceptance_criteria`
  - [x] `github_owner`, `github_repo`, `github_issue_number`, `github_pr_number?`
  - [x] `created_at`, `updated_at`, `claimed_by`, `completed_by`
- [x] Add transition validator for allowed role graph
- [x] Add status validator: `queued -> claimed -> completed/rejected`

## M2: GitHub Read Integration

- [x] Add GitHub client abstraction in hub
- [x] Validate issue exists at handoff creation
- [ ] Pull issue metadata (title/state/labels/assignees)
- [ ] Cache metadata for dashboard cards

## M3: Queue Operations

- [x] `POST /handoffs` (create)
- [x] `GET /handoffs` (filter by role/status/repo/issue)
- [x] `POST /handoffs/:id/claim`
- [x] `POST /handoffs/:id/complete`
- [x] `POST /handoffs/:id/promote` (advance to next role)
- [x] Concurrency guard: single successful claim

## M4: GitHub Write-backs

- [x] On claim: post issue comment
- [x] On complete: post issue comment with outcome
- [ ] On promote: update labels/assignee by role mapping
- [ ] Idempotency keys on outbound writes

## M5: Sync Engine

- [ ] Webhook endpoint with signature verification
- [ ] Idempotent event processing via delivery ID
- [ ] Reconciliation job (5–15 min) with watermark checkpoint
- [x] Drift detection + `needs_attention` state
- [ ] Admin/manual resync endpoint or CLI command

## M6: UX

- [ ] CLI:
  - [ ] `fmux handoff create --issue <n> --role-to <role>`
  - [ ] `fmux handoff list --role <role> --status <status>`
  - [ ] `fmux handoff claim <id>`
  - [ ] `fmux handoff complete <id> --outcome <...>`
  - [ ] `fmux handoff promote <id>`
- [ ] Dashboard:
  - [ ] Role queue views
  - [ ] Handoff card with GitHub context
  - [ ] Claim/Complete/Promote actions
  - [ ] Deep links to issue/PR/session

## M7: BDD (Red/Green)

- [x] Add/extend scenarios in `docs/specs/scenarios.md` for role workflow + GitHub-linked handoffs
- [x] Add/extend test coverage mapping in `TEST_PLAN.md` traceability matrix
- [x] Create handoff from valid GitHub issue
- [x] Reject handoff for unknown issue
- [x] Reviewer claim lock (second claim fails)
- [x] Complete review with approve -> promote to SRE queue
- [x] Request changes -> returns to implementer queue
- [x] Webhook issue close updates item state
- [ ] Reconciliation heals missed webhook event

## Definition of Done

- Role handoff lifecycle works end-to-end across CLI + dashboard
- GitHub-linked work items are visible and actionable in queues
- Webhook + reconciliation keep sync stable under failure
- BDD scenarios pass in CI
- `docs/specs/scenarios.md` and `TEST_PLAN.md` are updated and referenced from this epic

## Risks and Mitigations

- GitHub API rate limits:
  - Mitigation: cache and conditional requests
- Webhook delivery gaps:
  - Mitigation: reconciliation watermark polling
- Role transition misuse:
  - Mitigation: strict transition validator + RBAC checks
- Duplicate external writes:
  - Mitigation: idempotency keys and delivery logs

## Tracking

- Epic owner: Platform/Workflow
- Target release train: v0.1.x (incremental behind feature flag)
- Suggested feature flag: `role_handoffs_github`
