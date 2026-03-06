# Forgemux Hub — Technical Design Review

**Date:** 2026-02-27
**Reviewer:** Gemini CLI
**Subject:** Architecture Review of `tech_design3.md` (v3.0) and supporting artifacts (`forgemux-hub-stories.md`, `forgemux-hub.jsx`)

## 1. Executive Summary

The proposed architecture for **Forgemux Hub (v3.0)** is a robust, modern specification for a high-density agent observability platform. The design successfully pivots from a generic "agent runner" to a specialized "Human-in-the-Loop" intervention system.

The transition from the high-level concepts in `tech_design.md` to the concrete implementation details in `tech_design3.md` (backed by the JSX prototype) demonstrates a clear maturity in the system's definition. The architecture is **APPROVED** for implementation, subject to the specific recommendations and risk mitigations outlined below.

---

## 2. Architecture Analysis

### 2.1 Frontend Strategy (React + Inline Design System)
*   **Strengths:** The decision to use a custom "Theme Object" with inline/CSS-in-JS styling (as seen in `forgemux-hub.jsx`) rather than a heavy UI framework (MUI, AntD) is excellent for this specific use case. It allows for the "High Data Density" requirement—giving the team pixel-perfect control over spacing and typography which is critical for the "Decision Queue" and "Timeline" views.
*   **Risk:** Extensive use of inline styles can lead to performance bottlenecks (React re-renders) and maintenance challenges as the app grows.
*   **Recommendation:** For the v1 implementation, strictly isolate UI primitives (`Card`, `Badge`, `Dot`) as shown in the JSX to contain styling logic. For v2, consider migrating the Theme Object to a zero-runtime CSS-in-JS solution (like `vanilla-extract` or `panda-css`) to maintain the developer experience without the runtime overhead.

### 2.2 Communication Protocol (Hybrid SSE + WebSocket)
*   **Strengths:** The separation of concerns is architecturally sound:
    *   **SSE (Server-Sent Events):** Used for unidirectional state updates (Risk scores, Budget, Queue). This is lighter and more firewall-friendly than WebSockets for pure state syncing.
    *   **WebSockets:** Reserved strictly for the high-bandwidth, bidirectional `Terminal View`.
*   **Analysis:** This hybrid approach prevents the "noisy" session updates from clogging the terminal control channel.

### 2.3 Data Model & Persistence
*   **Strengths:** The `Decision` entity is well-modeled, specifically the separation of `repo` (primary) vs `impactRepos` (cascading). This explicitly handles the multi-repo complexity.
*   **Gap:** The specific database technology is left open in v3.0 (`tech_design.md` suggested SQLite/Postgres).
*   **Recommendation:** Given the relational nature of `Workspaces`, `Users`, and `Decisions`, **PostgreSQL** is the strongly recommended backing store. The JSON-heavy nature of "Session Replay" logs suggests using Postgres's `JSONB` column support, which allows efficient querying of unstructured logs without a separate NoSQL store in v1.

---

## 3. Critical Risks & Mitigations

### 3.1 The "Atomic Merge" Complexity
*   **Risk:** The design specifies an "Atomic Merge" feature that creates N synchronized PRs across N repositories. This is a distributed transaction problem. If PR #1 succeeds but PR #2 fails (e.g., merge conflict, permissions), the system enters an inconsistent state.
*   **Mitigation:** The backend must implement a "Saga" pattern or a robust state machine for these operations. It cannot be a simple fire-and-forget async loop. The UI must explicitly handle partial failure states (e.g., "2/3 PRs created, retry the 3rd?").

### 3.2 Edge Node Synchronization
*   **Risk:** The architecture relies on "Edge Nodes" (forged) to execute sessions. If an edge node disconnects or crashes, the Hub's "Live" indicator and session state must reflect this immediately.
*   **Mitigation:** Implement a strict **Heartbeat Protocol**. Edge nodes must ping the Hub every 5-10 seconds. The Hub must mark sessions as `unreachable` (greyed out in UI) if 2 heartbeats are missed, distinct from `blocked` or `error`.

### 3.3 Large Session Replay Payloads
*   **Risk:** Fetching the full timeline and logs for a long-running session (e.g., 6 hours, thousands of lines) via a single REST call (`GET /sessions/:id/replay/logs`) will choke the client.
*   **Mitigation:**
    1.  **Pagination:** The log API must support cursor-based pagination.
    2.  **Summary Level:** The "Timeline" should initially load only high-level events (System, Decision, Switch). Tool outputs (stdout) should be lazy-loaded only when the user expands a specific node or enters the "Terminal" tab.

---

## 4. UX/UI Refinement Recommendations

Based on the `forgemux-hub.jsx` prototype:

1.  **Mobile Responsiveness:** The current "Sidebar + Main Pane" layout in the Session Replay view will break on smaller screens. *Recommendation: Hide the Timeline Sidebar behind a toggle on viewports < 1024px.*
2.  **Typography:** The "Three-Font System" is distinct but requires careful loading strategy to prevent FOUT (Flash of Unstyled Text). Ensure `Poppins`, `Outfit`, and `JetBrains Mono` are preloaded or use `font-display: swap`.

---

## 5. Conclusion

The design is **High Quality**. It solves the specific "Alert Fatigue" problem with a novel "Decision Queue" approach rather than just building another generic terminal runner.

**Next Steps:**
1.  **Database Schema Finalization:** Define the Postgres schema for `Decisions` and `Workspaces`.
2.  **API Contract:** Swagger/OpenAPI spec for the Hub <-> Edge communication.
3.  **Prototype:** Proceed with the React implementation as defined in `tech_design3.md`.
