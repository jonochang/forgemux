Based on the strategic analysis in the reports, the core UX problem in the AI agent market is **cognitive overload and lack of trust**. Developers are currently forced to read thousands of lines of raw terminal output to understand what an agent did, and they are overwhelmed by constant notifications from multiple agents running in parallel.

Here are the top 3 UX features Forgemux must build to solve this, followed by a brief for your UX Designer.

### The Top 3 UX Features
1. **The Decision Queue (Structured Human-in-the-Loop):** Moving away from forcing users to open a raw terminal just to answer a simple "Yes/No" prompt. When an agent is blocked, the UI should extract the context and present a clean, actionable "Approve / Deny / Edit" card in an inbox-style queue.
2. **Differential Session Replay & Multi-Modal Attach:** Replacing raw text logs with a visual, interactive timeline of the agent’s actions (e.g., `[0:12] Created auth.ts` -> `[0:18] Ran tests (Failed)` -> `[0:20] Fixed auth.ts`). Users should be able to view diffs and file trees, not just a terminal shell. 
3. **Attention Budgeting & Risk Heatmap:** A smart dashboard that limits notifications based on a user's configured "budget" (e.g., max 5 interruptions a day) and uses visual indicators (Risk Scores, Context Window Pressure) to highlight which agents actually need human supervision.

---

# UX Design Brief: Forgemux Web Dashboard

## 1. Project Overview & Vision
**Forgemux** is an enterprise-grade infrastructure platform for running AI coding agents (like Claude Code or AutoGPT) at scale. We handle the complex backend wiring (sessions, durable memory, security). 

**The UX Goal:** We need to build a lightweight, web-based dashboard on top of this infrastructure. We are *not* building a project management tool (no Kanban boards, no Jira tickets). We are building an **Agent Observability and Intervention Center**. The design must make managing 20-50 simultaneous AI agents feel as calm and organized as reviewing a pull request.

## 2. The User Profile
* **Target Audience:** Senior Software Engineers, Platform Engineers, and Tech Leads.
* **Their Current Pain Points:** 
  * *Notification Fatigue:* Agents constantly ping them for trivial things.
  * *The "Black Box" Problem:* To see what an agent is doing, they have to read a massive wall of scrolling terminal text.
  * *Context Switching:* Having to drop their own coding work to babysit a stuck agent terminal.
* **Design Aesthetic:** High data density, developer-native (dark mode default, monospaced typography for code, keyboard-shortcut friendly), clean, and utilitarian. Think GitHub Actions meets Linear.

## 3. Top 3 Core Features to Design

### Feature 1: The "Decision Queue" (Intervention Inbox)
Currently, when an AI agent needs permission to run a command or write a file, it stops in a terminal and waits for human input. We are extracting this into a structured UI.
* **What to design:** An inbox-style list of pending decisions.
* **Anatomy of a Decision Card:** 
  * Which agent/session is asking?
  * The actual question (e.g., *"Are you sure you want to run `npm install next@latest`?"*)
  * Context snippet (a small diff or the last 3 lines of the log).
  * Action buttons: **Approve**, **Deny**, or a **Comment input box** to give the agent new instructions.
* **UX Goal:** A developer should be able to clear 5 pending agent decisions in 15 seconds without ever opening a terminal.

### Feature 2: Multi-Modal Session Replay (The "What Happened?" Timeline)
We need a detail view for a specific agent session. Do not just show a giant black box terminal. 
* **What to design:** A multi-pane layout for inspecting an agent’s work.
* **Core Elements:**
  * **The Timeline (Left pane or top bar):** A visual progression of what the agent did. Instead of raw text, show semantic nodes: *Prompted -> Created 3 files -> Ran test (Failed) -> Fixed code -> Ran test (Passed).*
  * **The Multi-Modal View (Main pane):** Tabs to switch between:
    1. *Diff View:* What code has the agent actually changed? (Crucial for trust).
    2. *File Tree:* What files exist in the agent's isolated workspace?
    3. *Structured Log:* A clean list of tool calls and outputs.
    4. *Terminal View:* The raw fallback view.
  * **"Fork Here" Action:** Allow users to hover over any point in the timeline and click a "Branch/Fork" button to spin up a new agent from that exact moment in time.

### Feature 3: The "Attention & Risk" Dashboard
This is the home screen. It needs to give the user a bird's-eye view of their AI fleet while protecting their time.
* **What to design:** A dashboard tracking active, queued, and completed sessions, augmented with health/risk data.
* **Core Elements:**
  * **Attention Budget Indicator:** A visual meter (e.g., a battery or progress bar) showing how many "interruptions" the user has left for the day. 
  * **Session Risk Heatmap:** Active sessions shouldn't just say "Running." They should have visual badges for context health.
    * *Green:* Low token usage, tests are passing.
    * *Yellow/Orange:* High Context Pressure (approaching token limit) or Edit Velocity (making lots of changes without running tests). 
  * **Smart Grouping:** Group active sessions by severity. A failed production deployment needs immediate attention; a routine dependency update can be batched into a "Review Later" bucket.

## 4. What NOT to Design (Anti-Goals)
* **No Project Management:** Do not design ticket trackers, goal hierarchies, or Eisenhower matrices. 
* **No Agent Config UIs:** Do not design complex sliders for "AI Temperature" or "Model Selection." Assume that is handled in config files. 
* **No Heavy Desktop UI Chrome:** Keep the interface feeling like a lightweight, fast web application. Focus on data, not heavy branding or illustrative graphics.
