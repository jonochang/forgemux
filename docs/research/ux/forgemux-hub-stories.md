# Forgemux Hub — User Stories & Feature Specification

**Product:** Forgemux Web Dashboard (Enterprise Workspace Edition)
**Version:** 0.1.0-alpha
**Last updated:** 2026-02-27
**Companion mockup:** `forgemux-final.jsx`

---

## Data Model Summary

All stories reference this hierarchy:

- **Organization** → top-level tenant (e.g. Silverpond)
- **Workspace** → team domain bundling multiple Git repos (e.g. "Checkout Experience")
- **Session (Agent)** → a single AI task running inside a Workspace, with access to all repos in that Workspace simultaneously
- **Decision** → a pause point where an agent needs human input before continuing

---

## Feature 1: Workspace Fleet Dashboard

The home screen when a user selects their Workspace. Provides a bird's-eye view of all agents, cost, risk, and team capacity at a glance.

### 1.1 Attention Budget Meter

> **As a** Tech Lead,
> **I want to** see how many decision requests my team has handled today versus our configured daily limit,
> **so that** I can gauge whether we have capacity for more agent work or need to slow down launches.

**Acceptance criteria:**

- Displays a visual meter (ring/gauge) showing `used / total` decisions for the current day.
- Color transitions from green → amber → red as the budget is consumed.
- Shows the remaining count as a prominent number ("5 decisions remaining").
- Includes a note that unresolved decisions pause agents, making the cost of ignoring the queue explicit.
- Budget total is configurable per-Workspace in settings.
- Resets at midnight in the Workspace's configured timezone.

### 1.2 Session Summary Stats

> **As a** Platform Engineer,
> **I want to** see a quick count of active, blocked, queued, and completed sessions plus total cost,
> **so that** I can understand fleet utilisation without scrolling through individual agents.

**Acceptance criteria:**

- Five stat cards displayed in a horizontal strip: Active, Blocked, Queued, Complete, Cost Today.
- Each card shows the count or dollar amount with the appropriate semantic color.
- Cost aggregates all sessions in the current Workspace for the current day.
- Numbers update in real-time (or near real-time via polling/WebSocket).

### 1.3 Session Risk Heatmap

> **As a** Tech Lead,
> **I want to** see all active and blocked sessions as a list with color-coded risk indicators,
> **so that** I can immediately identify which agents need intervention.

**Acceptance criteria:**

- Each session row displays: risk color (green/yellow/red) as a left border and indicator dot, session goal as the primary label, status badge, pending decision count, cross-repo impact pills, context health bar with percentage, test status badge, token count, cost, and uptime.
- **Risk scoring logic:**
  - **Green:** Context < 70%, tests passing across all touched repos, no pending decisions older than 15 minutes.
  - **Yellow:** Context 70–85%, OR tests failing in any repo, OR pending decisions between 5–15 minutes old.
  - **Red:** Context > 85%, OR agent is looping (>3 consecutive failed tool calls), OR pending decisions older than 15 minutes, OR agent explicitly blocked.
- Clicking a session row navigates to the Session Replay view for that session.
- Red-risk sessions pulse their indicator dot to draw attention.

### 1.4 Cross-Repo Impact Pills

> **As a** Senior Developer who owns `payment-gateway`,
> **I want to** see which repositories each agent is actively modifying,
> **so that** I know at a glance if an agent is touching my code.

**Acceptance criteria:**

- Each Workspace repo has a unique icon and color (e.g. `◈ payment-gateway` in green, `◐ frontend-web` in blue).
- Every session card displays repo pills for all repos the session has modified.
- Repo identity (icon + color) is consistent across all views in the dashboard.
- Repos are displayed even if the session has only read from them (with a visual distinction from write access, future iteration).

### 1.5 Queued & Completed Panels

> **As a** Tech Lead,
> **I want to** see what work is waiting to start and what finished recently,
> **so that** I can plan my review time and understand throughput.

**Acceptance criteria:**

- Two side-by-side panels below the heatmap: "Queued" and "Recently Completed."
- Queued sessions show: goal, target repos, assigned model.
- Completed sessions show: goal, repos touched, lines added/removed, cost, and duration.
- Completed sessions link to their Session Replay for post-mortem review.

---

## Feature 2: Decision Queue

An inbox-style interface for resolving agent pause points. Designed for a Tech Lead to clear 5+ decisions in under a minute.

### 2.1 Decision List with Repo Filtering

> **As a** Tech Lead who owns `payment-gateway`,
> **I want to** filter the decision queue to only show decisions from repos I'm responsible for,
> **so that** I don't waste time reading decisions meant for the frontend team.

**Acceptance criteria:**

- Filter bar at the top with buttons for "All repos" and one button per Workspace repo.
- Filtering is instant (client-side) with no page reload.
- Filter state persists within the session but does not affect other users.
- The pending count in the navigation badge reflects the unfiltered total.

### 2.2 Decision Card Anatomy

> **As a** Tech Lead,
> **I want to** see the agent's question, which repo it's working in, the severity, and enough context to make a decision — all without expanding the card,
> **so that** I can approve or deny quickly.

**Acceptance criteria:**

- Each decision card displays (collapsed state): severity dot with color and pulse for critical, repo pill (primary repo), cross-repo impact chain if applicable (e.g. `shared-protobufs → payment-gateway → frontend-web`), severity badge, tag badges, the agent's question as primary text, agent ID, agent goal, age, and assigned reviewer.
- Approve, Deny, and Comment buttons are always visible without expanding.
- A severity stripe at the top of the card provides color-at-a-glance: critical = solid red, high = amber, medium = blue, low = gray.

### 2.3 Decision Context Expansion

> **As a** Senior Developer,
> **I want to** expand a decision card to see the diff, log output, or screenshot the agent is referencing,
> **so that** I can make an informed decision without switching to a terminal.

**Acceptance criteria:**

- Clicking anywhere on the card (except action buttons) toggles the expanded context panel.
- Context types supported: diff (with syntax-highlighted add/remove lines and file path), log output (monospaced, colored by severity), and screenshot/description (italic text with placeholder for future image support).
- Diff context shows the file path above the diff block.
- Only one card can be expanded at a time.

### 2.4 Severity-Based Ordering and Escalation

> **As a** Platform Engineer,
> **I want** critical decisions to be visually dominant and sorted to the top,
> **so that** database migrations and security-related questions don't get buried.

**Acceptance criteria:**

- Decisions are sorted by severity (critical → high → medium → low), then by age (oldest first within each severity).
- Critical decisions have a full-opacity red stripe and a pulsing severity dot.
- The header shows total pending count and calls out critical count separately in red.
- Future iteration: critical decisions older than 10 minutes trigger a notification to the assigned reviewer.

### 2.5 Quick Actions

> **As a** Tech Lead,
> **I want to** approve, deny, or comment on a decision with a single click,
> **so that** I can clear the queue fast and unblock agents.

**Acceptance criteria:**

- **Approve** unblocks the agent and lets it proceed with the proposed approach.
- **Deny** unblocks the agent with a signal to try an alternative approach or stop.
- **Comment** opens an inline text input for the reviewer to provide guidance without approving or denying. The agent receives the comment and continues working.
- All three actions record the reviewer's identity and timestamp.
- After an action, the card animates out of the list.
- Keyboard shortcuts (future iteration): `a` to approve, `d` to deny, `c` to comment on the focused card.

### 2.6 Cross-Repo Impact Visibility

> **As a** Tech Lead,
> **I want to** see when a decision in one repo will cascade changes into other repos,
> **so that** I can pull in the right reviewers or flag the decision for wider discussion.

**Acceptance criteria:**

- If an agent's proposed change impacts repos beyond the one it's currently editing, the card shows an impact chain: `primary-repo → impacted-repo-1 → impacted-repo-2`.
- Impact repos are displayed as repo pills with arrow separators.
- This is determined by the agent's declared plan (which repos it intends to modify next).

---

## Feature 3: Multi-Repo Session Replay

A detail view for inspecting what an agent did, is doing, or plans to do — across multiple codebases simultaneously.

### 3.1 Timeline Sidebar with Cross-Repo Context Switches

> **As a** Senior Developer reviewing a completed session,
> **I want to** see a chronological timeline of the agent's actions with clear markers for when it switched between repositories,
> **so that** I can trace the logical flow of a cross-repo change.

**Acceptance criteria:**

- Left sidebar displays a vertical timeline of events.
- Each event shows: timestamp, repo icon + name (color-coded), and action description.
- Context switches between repos are visually distinct: larger marker, colored connector line, bold text.
- Decision requests are highlighted with a red background in the timeline.
- Event types: system, read, edit, tool (bash/CLI), context switch, test (with pass/fail result), decision.
- Clicking a timeline event scrolls the main pane to the corresponding change.

### 3.2 Unified Diff View (Grouped by Repository)

> **As a** Senior Developer,
> **I want to** see all file changes grouped by repository with clear visual boundaries,
> **so that** I can review cross-repo changes in a logical structure rather than a flat list.

**Acceptance criteria:**

- Files are grouped under repo headers with the repo's icon, color, and name.
- Each repo header shows the count of files changed.
- Individual files show: file path, additions count (green), deletions count (red).
- Clicking a file expands it to show the full diff (future iteration).
- The grouping mirrors the order the agent touched the repos in.

### 3.3 Multi-Repo File Tree

> **As a** Senior Developer,
> **I want to** browse the agent's working directory across all repos in the Workspace,
> **so that** I can understand the file structure and find changes I might have missed in the diff.

**Acceptance criteria:**

- Tree view showing only repos that the session modified.
- Each repo is a top-level folder with its icon and color.
- Files the agent modified are highlighted or badged.
- Clicking a file opens its diff in the main pane.

### 3.4 Structured Log View

> **As a** Platform Engineer debugging a failed session,
> **I want to** see a clean, tabular log of every tool call the agent made,
> **so that** I can identify where things went wrong without reading raw terminal output.

**Acceptance criteria:**

- Table with columns: timestamp, event type icon, repo, action description, result badge (pass/fail where applicable).
- Alternating row backgrounds for scanability.
- Filterable by event type (future iteration).
- Context switches and decisions are visually emphasized.

### 3.5 Raw Terminal View

> **As a** Senior Developer who wants the full picture,
> **I want to** see the raw terminal output of the agent session,
> **so that** I have a fallback when the structured views don't show enough detail.

**Acceptance criteria:**

- Monospaced, dark background terminal display.
- Color-coded output: user prompts (ember), tool calls (ember glow), success (green), errors (red), context switches (purple), system info (gray).
- Scrollable with the full session history.

### 3.6 Atomic Merge Action

> **As a** Tech Lead who has reviewed a completed multi-repo session,
> **I want to** generate synchronized Pull Requests across all modified repositories with a single action,
> **so that** the cross-repo change lands atomically and the PRs are linked together.

**Acceptance criteria:**

- Primary CTA button in the tab bar: "Atomic Merge → N PRs" where N is the count of repos with changes.
- Clicking opens a confirmation showing: list of repos, branch names, file counts, and target branches.
- On confirm, creates one PR per modified repo, all linked together via a shared reference ID.
- PRs are created in the connected GitHub/GitLab instance with a description noting the cross-repo context and linking to sibling PRs.
- If any PR creation fails, the user is notified and can retry individually.

---

## Feature 4: Navigation & Workspace Context

### 4.1 Organization / Workspace Breadcrumb

> **As a** user who belongs to multiple Workspaces,
> **I want to** see which Organization and Workspace I'm currently viewing and switch between them,
> **so that** I never accidentally act on the wrong team's agents.

**Acceptance criteria:**

- Top navigation shows `Organization / Workspace` as a breadcrumb.
- Workspace name is in a clickable chip that opens a Workspace switcher dropdown.
- All data on the page is scoped to the selected Workspace.

### 4.2 Repository Identity System

> **As a** developer working across multiple repos,
> **I want** each repository to have a consistent icon and color across all views,
> **so that** I can instantly recognize which repo is being referenced without reading the name.

**Acceptance criteria:**

- Each repo in a Workspace is assigned a unique icon (from a fixed set: `◐`, `◈`, `◇`, `◎`, etc.) and a unique color.
- The `RepoPill` component is used everywhere a repo is referenced: decision cards, session cards, timeline events, diff headers, file tree roots.
- Repo icons appear in a compact strip in the top navigation as a passive reminder of the Workspace scope.

### 4.3 Pending Decision Badge in Navigation

> **As a** Tech Lead,
> **I want to** see the pending decision count in the navigation without visiting the Decisions tab,
> **so that** I know when agents are waiting for me.

**Acceptance criteria:**

- The "Decisions" nav tab shows a count badge when pending decisions > 0.
- Badge uses the ember accent color for visibility.
- Count updates in real-time.

### 4.4 Live Indicator

> **As a** user,
> **I want to** see a live connection indicator in the top navigation,
> **so that** I trust the data on screen is current.

**Acceptance criteria:**

- Green dot + "live" label in the top-right of the navigation bar.
- If the WebSocket/polling connection drops, the indicator changes to amber "reconnecting" or red "disconnected."

---

## Feature 5: Typography & Design System

### 5.1 Three-Font System

> **As a** designer maintaining the Forgemux design system,
> **I want** a clear rule for when to use each font,
> **so that** the UI is consistent and contributors don't have to guess.

**Font assignments:**

| Token    | Font           | Weights  | Usage                                                       |
|----------|----------------|----------|-------------------------------------------------------------|
| `T.sans` | Poppins        | 300–500  | Headers (500), body text (400), labels, button text, nav    |
| `T.data` | Outfit         | 300–500  | Stat numbers (300), costs, token counts, percentages, diffs |
| `T.mono` | JetBrains Mono | 400–600  | Session IDs, branch names, file paths, terminal, code       |

**Rules:**

- If it's a **number or metric** a human scans rather than reads → `T.data` (Outfit).
- If it's **words a human reads** → `T.sans` (Poppins).
- If it's **code, an identifier, or a path** → `T.mono` (JetBrains Mono).
- Nothing above weight **500** for Poppins or Outfit. The only 600 is `T.mono` in section labels and the "forgemux" brand mark.

### 5.2 Color Semantics

| Token      | Hex       | Meaning                              |
|------------|-----------|--------------------------------------|
| `T.ember`  | `#E8622C` | Primary brand, CTAs, active tab      |
| `T.ok`     | `#34D399` | Success, active, tests passing       |
| `T.warn`   | `#FBBF24` | Warning, idle, attention needed      |
| `T.err`    | `#F87171` | Error, blocked, tests failing        |
| `T.info`   | `#60A5FA` | Informational, complete, neutral     |
| `T.purple` | `#A78BFA` | Merged, Opus model indicator         |
| `T.molten` | `#FF9F43` | High severity, cost values, burn     |

---

## Non-Functional Requirements

- **No single-repo assumptions.** Every view must accommodate sessions that touch 1–N repos. The UI must never assume N=1.
- **No project management tooling.** Forgemux consumes tasks from external systems; it does not manage them. No Kanban boards, sprint planners, or ticket trackers.
- **Data density over chrome.** Optimize for scan speed. A Tech Lead should be able to assess fleet health in under 5 seconds and clear 5 decisions in under 60 seconds.
- **Keyboard-first (future iteration).** All primary actions should be reachable via keyboard shortcuts. Navigation between views via `1/2/3` keys. Decision actions via `a/d/c`.
- **Real-time updates.** All counts, statuses, and risk scores should update without manual refresh. WebSocket preferred, polling acceptable with ≤5s intervals.
- **Dark mode default.** Light mode is a non-goal for v1.
