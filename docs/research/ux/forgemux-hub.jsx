import { useState, useRef, useEffect } from "react";

/* ═══════════════════════════════════════════
   THEME
   ═══════════════════════════════════════════ */
const T = {
  bg0: "#0A0A0C", bg1: "#101013", bg2: "#161619", bg3: "#1E1E23", bg4: "#26262D",
  border: "#28282F", borderS: "#1C1C22",
  t1: "#E4E2DF", t2: "#98968F", t3: "#5A5955", t4: "#3A3937",
  ember: "#E8622C", emberG: "#F0884D", emberS: "rgba(232,98,44,0.08)",
  molten: "#FF9F43",
  ok: "#34D399", okD: "#22C55E", okS: "rgba(52,211,153,0.08)", okSolid: "rgba(52,211,153,0.15)",
  warn: "#FBBF24", warnS: "rgba(251,191,36,0.08)",
  err: "#F87171", errD: "#EF4444", errS: "rgba(248,113,113,0.08)",
  info: "#60A5FA", infoS: "rgba(96,165,250,0.08)",
  purple: "#A78BFA", purpleS: "rgba(167,139,250,0.08)",
  cyan: "#22D3EE", cyanS: "rgba(34,211,238,0.08)",
  mono: "'JetBrains Mono', monospace",
  sans: "'Poppins', sans-serif",
  data: "'Outfit', sans-serif",
};

/* ═══════════════════════════════════════════
   DATA — WORKSPACE CONTEXT
   ═══════════════════════════════════════════ */
const workspace = {
  name: "Checkout Experience",
  org: "Silverpond",
  repos: [
    { id: "frontend-web", label: "frontend-web", color: T.info, icon: "◐" },
    { id: "payment-gateway", label: "payment-gateway", color: T.ok, icon: "◈" },
    { id: "shared-protobufs", label: "shared-protobufs", color: T.purple, icon: "◇" },
    { id: "checkout-service", label: "checkout-service", color: T.molten, icon: "◎" },
  ],
  members: ["jono", "rowan", "alex", "sam"],
  attentionBudget: { used: 7, total: 12 },
};

const decisions = [
  {
    id: "D-0041", agentId: "S-9593", agentGoal: "Add Stripe webhook signature verification",
    repo: "payment-gateway", question: "The webhook secret is currently hardcoded. Should I move it to environment variables or use the existing Vault integration?",
    context: { type: "diff", file: "src/webhooks/verify.rs", lines: [
      { type: "ctx", text: "fn verify_signature(payload: &[u8], sig: &str) -> Result<()> {" },
      { type: "del", text: '    let secret = "whsec_test_abc123";' },
      { type: "add", text: '    let secret = std::env::var("STRIPE_WEBHOOK_SECRET")?;' },
    ]},
    severity: "high", assignedTo: "jono", age: "3m",
    tags: ["security", "config"],
  },
  {
    id: "D-0040", agentId: "S-1077", agentGoal: "Implement OAuth2 PKCE flow",
    repo: "frontend-web", question: "The design system has two modal components: <Dialog> and <Sheet>. Which should I use for the OAuth consent screen?",
    context: { type: "screenshot", description: "Agent found two components and is unsure which to use for the consent flow." },
    severity: "medium", assignedTo: "rowan", age: "12m",
    tags: ["design", "ux"],
  },
  {
    id: "D-0039", agentId: "S-a82f", agentGoal: "Update protobuf schema for new payment fields",
    repo: "shared-protobufs", question: "Adding `recurring_interval` field to PaymentIntent. This will require regenerating bindings in payment-gateway and frontend-web. Should I proceed with the cross-repo update?",
    context: { type: "diff", file: "proto/payment/v1/intent.proto", lines: [
      { type: "ctx", text: "message PaymentIntent {" },
      { type: "ctx", text: "  string currency = 3;" },
      { type: "add", text: "  RecurringInterval recurring_interval = 7;" },
      { type: "add", text: "}" },
      { type: "add", text: "" },
      { type: "add", text: "enum RecurringInterval {" },
      { type: "add", text: "  MONTHLY = 0;" },
      { type: "add", text: "  YEARLY = 1;" },
    ]},
    severity: "high", assignedTo: "jono", age: "28m",
    tags: ["schema", "cross-repo"],
    impactRepos: ["payment-gateway", "frontend-web"],
  },
  {
    id: "D-0038", agentId: "S-c44d", agentGoal: "Add error boundary to checkout flow",
    repo: "frontend-web", question: "The Sentry SDK is v7 but the error boundary API changed in v8. Should I upgrade Sentry or use the v7 API?",
    context: { type: "log", text: "npm warn deprecated @sentry/react@7.x: Please upgrade to v8" },
    severity: "low", assignedTo: null, age: "1h",
    tags: ["deps"],
  },
  {
    id: "D-0037", agentId: "S-9593", agentGoal: "Add Stripe webhook signature verification",
    repo: "payment-gateway", question: "Should I add a DB migration to store webhook delivery logs? This would add a new table `webhook_events` for idempotency tracking.",
    context: { type: "diff", file: "migrations/20260227_webhook_events.sql", lines: [
      { type: "add", text: "CREATE TABLE webhook_events (" },
      { type: "add", text: "  id UUID PRIMARY KEY DEFAULT gen_random_uuid()," },
      { type: "add", text: "  event_id TEXT UNIQUE NOT NULL," },
      { type: "add", text: "  processed_at TIMESTAMPTZ DEFAULT NOW()" },
      { type: "add", text: ");" },
    ]},
    severity: "critical", assignedTo: "jono", age: "45m",
    tags: ["database", "migration"],
  },
];

const sessions = [
  {
    id: "S-9593", goal: "Add Stripe webhook signature verification",
    status: "active", risk: "green", model: "Sonnet 4.5",
    repos: ["payment-gateway"], context: 58, tokens: "52.7k", cost: 0.34,
    uptime: "1h 23m", commits: 4, linesAdded: 312, linesRemoved: 41,
    pendingDecisions: 2, testsStatus: "passing",
  },
  {
    id: "S-1077", goal: "Implement OAuth2 PKCE flow",
    status: "active", risk: "yellow", model: "Sonnet 4.5",
    repos: ["frontend-web", "payment-gateway"], context: 71, tokens: "83.7k", cost: 0.54,
    uptime: "2h 08m", commits: 3, linesAdded: 218, linesRemoved: 12,
    pendingDecisions: 1, testsStatus: "failing",
  },
  {
    id: "S-a82f", goal: "Update protobuf schema for new payment fields",
    status: "active", risk: "yellow", model: "Opus 4.6",
    repos: ["shared-protobufs", "payment-gateway", "frontend-web"], context: 44, tokens: "35.1k", cost: 1.22,
    uptime: "45m", commits: 1, linesAdded: 67, linesRemoved: 0,
    pendingDecisions: 1, testsStatus: "pending",
  },
  {
    id: "S-c44d", goal: "Add error boundary to checkout flow",
    status: "blocked", risk: "red", model: "Sonnet 4.5",
    repos: ["frontend-web"], context: 89, tokens: "128.4k", cost: 0.82,
    uptime: "4h 12m", commits: 6, linesAdded: 89, linesRemoved: 34,
    pendingDecisions: 1, testsStatus: "failing",
  },
  {
    id: "S-f19b", goal: "Generate Go bindings from updated protos",
    status: "queued", risk: "green", model: "Sonnet 4.5",
    repos: ["payment-gateway", "shared-protobufs"], context: 0, tokens: "0", cost: 0,
    uptime: "—", commits: 0, linesAdded: 0, linesRemoved: 0,
    pendingDecisions: 0, testsStatus: "none",
  },
  {
    id: "S-d7e4", goal: "Sync checkout UI with new payment fields",
    status: "queued", risk: "green", model: "Sonnet 4.5",
    repos: ["frontend-web"], context: 0, tokens: "0", cost: 0,
    uptime: "—", commits: 0, linesAdded: 0, linesRemoved: 0,
    pendingDecisions: 0, testsStatus: "none",
  },
  {
    id: "S-e331", goal: "Migrate legacy Braintree handlers",
    status: "complete", risk: "green", model: "Opus 4.6",
    repos: ["payment-gateway", "checkout-service"], context: 0, tokens: "201.4k", cost: 2.91,
    uptime: "6h 31m", commits: 14, linesAdded: 1243, linesRemoved: 876,
    pendingDecisions: 0, testsStatus: "passing",
  },
];

const replayTimeline = [
  { time: "0:00", action: "Session started", repo: null, type: "system" },
  { time: "0:02", action: "Read PaymentIntent proto schema", repo: "shared-protobufs", type: "read" },
  { time: "0:05", action: "Added recurring_interval field to intent.proto", repo: "shared-protobufs", type: "edit" },
  { time: "0:08", action: "Ran protoc to generate Rust bindings", repo: "shared-protobufs", type: "tool" },
  { time: "0:12", action: "Switched context to payment-gateway", repo: "payment-gateway", type: "switch" },
  { time: "0:14", action: "Updated PaymentService to handle new field", repo: "payment-gateway", type: "edit" },
  { time: "0:18", action: "Ran cargo test — 2 failures", repo: "payment-gateway", type: "test", result: "fail" },
  { time: "0:22", action: "Fixed type mismatch in handler", repo: "payment-gateway", type: "edit" },
  { time: "0:24", action: "Ran cargo test — all passing", repo: "payment-gateway", type: "test", result: "pass" },
  { time: "0:28", action: "Switched context to frontend-web", repo: "frontend-web", type: "switch" },
  { time: "0:31", action: "Updated TypeScript types from proto", repo: "frontend-web", type: "edit" },
  { time: "0:35", action: "Modified CheckoutForm component", repo: "frontend-web", type: "edit" },
  { time: "0:38", action: "Decision requested: Cross-repo update scope", repo: "shared-protobufs", type: "decision" },
];

/* ═══════════════════════════════════════════
   SHARED COMPONENTS
   ═══════════════════════════════════════════ */
const Dot = ({ color, size = 7, pulse = false }) => (
  <span style={{
    display: "inline-block", width: size, height: size, borderRadius: "50%",
    background: color, flexShrink: 0,
    boxShadow: pulse ? `0 0 8px ${color}55` : "none",
  }} />
);

const Badge = ({ children, color = T.t2, bg = T.bg4, style: s = {} }) => (
  <span style={{
    fontSize: 10, fontFamily: T.mono, fontWeight: 500, padding: "2px 7px",
    borderRadius: 4, background: bg, color, border: `1px solid ${color}18`,
    textTransform: "uppercase", letterSpacing: "0.04em", whiteSpace: "nowrap",
    lineHeight: "16px", ...s,
  }}>{children}</span>
);

const RepoPill = ({ repoId }) => {
  const r = workspace.repos.find(rr => rr.id === repoId);
  if (!r) return null;
  return (
    <span style={{
      fontSize: 11, fontFamily: T.mono, fontWeight: 500, padding: "2px 8px",
      borderRadius: 4, background: `${r.color}10`, color: r.color,
      border: `1px solid ${r.color}18`, whiteSpace: "nowrap",
      display: "inline-flex", alignItems: "center", gap: 4,
    }}>
      <span style={{ fontSize: 10 }}>{r.icon}</span> {r.label}
    </span>
  );
};

const MiniBar = ({ value, max = 100, color = T.ember, h = 3 }) => (
  <div style={{ width: "100%", height: h, borderRadius: h, background: T.bg4, overflow: "hidden" }}>
    <div style={{ width: `${Math.min((value / max) * 100, 100)}%`, height: "100%", borderRadius: h, background: color, transition: "width 0.3s" }} />
  </div>
);

const SectionLabel = ({ children }) => (
  <div style={{ fontSize: 10, fontFamily: T.mono, fontWeight: 600, letterSpacing: "0.1em", textTransform: "uppercase", color: T.t3, marginBottom: 8 }}>
    {children}
  </div>
);

const Card = ({ children, style: s = {} }) => (
  <div style={{ background: T.bg1, borderRadius: 8, border: `1px solid ${T.borderS}`, ...s }}>
    {children}
  </div>
);

const riskColor = (r) => ({ green: T.ok, yellow: T.warn, red: T.err }[r] || T.t3);
const statusColor = (s) => ({ active: T.ok, blocked: T.err, queued: T.t3, complete: T.info }[s] || T.t3);
const contextColor = (v) => v < 40 ? T.ok : v < 70 ? T.molten : v < 85 ? T.warn : T.err;
const severityColor = (s) => ({ critical: T.errD, high: T.molten, medium: T.info, low: T.t3 }[s]);

/* ═══════════════════════════════════════════
   FEATURE 1: DECISION QUEUE
   ═══════════════════════════════════════════ */
const DecisionQueue = () => {
  const [repoFilter, setRepoFilter] = useState("all");
  const [expanded, setExpanded] = useState(null);

  const filtered = decisions.filter(d => repoFilter === "all" || d.repo === repoFilter);
  const critCount = decisions.filter(d => d.severity === "critical").length;

  const DiffBlock = ({ lines }) => (
    <div style={{ background: T.bg0, borderRadius: 6, border: `1px solid ${T.borderS}`, overflow: "hidden", fontFamily: T.mono, fontSize: 12, lineHeight: 1.7 }}>
      {lines.map((l, i) => (
        <div key={i} style={{
          padding: "1px 12px",
          background: l.type === "add" ? "rgba(52,211,153,0.06)" : l.type === "del" ? "rgba(248,113,113,0.06)" : "transparent",
          color: l.type === "add" ? T.ok : l.type === "del" ? T.err : T.t3,
          borderLeft: l.type === "add" ? `2px solid ${T.ok}44` : l.type === "del" ? `2px solid ${T.err}44` : "2px solid transparent",
        }}>
          <span style={{ color: T.t4, marginRight: 8, userSelect: "none" }}>
            {l.type === "add" ? "+" : l.type === "del" ? "−" : " "}
          </span>
          {l.text}
        </div>
      ))}
    </div>
  );

  return (
    <div style={{ padding: 24, maxWidth: 960, margin: "0 auto" }}>
      {/* Header */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 20 }}>
        <div>
          <h2 style={{ fontSize: 18, fontWeight: 500, color: T.t1, fontFamily: T.sans, margin: 0, letterSpacing: "-0.01em" }}>
            Decision Queue
          </h2>
          <div style={{ fontSize: 12, color: T.t3, fontFamily: T.sans, marginTop: 4 }}>
            {decisions.length} pending · {critCount > 0 && <span style={{ color: T.err }}>{critCount} critical</span>}
          </div>
        </div>
        <div style={{ display: "flex", gap: 4, alignItems: "center" }}>
          <button onClick={() => setRepoFilter("all")} style={{
            background: repoFilter === "all" ? T.bg4 : "transparent",
            color: repoFilter === "all" ? T.t1 : T.t3,
            border: `1px solid ${repoFilter === "all" ? T.border : "transparent"}`,
            borderRadius: 6, padding: "5px 12px", fontSize: 11, cursor: "pointer", fontFamily: T.sans,
          }}>All repos</button>
          {workspace.repos.map(r => (
            <button key={r.id} onClick={() => setRepoFilter(r.id)} style={{
              background: repoFilter === r.id ? `${r.color}12` : "transparent",
              color: repoFilter === r.id ? r.color : T.t3,
              border: `1px solid ${repoFilter === r.id ? `${r.color}30` : "transparent"}`,
              borderRadius: 6, padding: "5px 10px", fontSize: 11, cursor: "pointer",
              fontFamily: T.mono, display: "flex", alignItems: "center", gap: 4,
            }}>
              <span style={{ fontSize: 10 }}>{r.icon}</span>
              {r.label}
            </button>
          ))}
        </div>
      </div>

      {/* Decision cards */}
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {filtered.map((d) => {
          const isOpen = expanded === d.id;
          const sc = severityColor(d.severity);
          return (
            <Card key={d.id} style={{ overflow: "hidden" }}>
              {/* Severity stripe */}
              <div style={{ height: 2, background: sc, opacity: d.severity === "critical" ? 1 : 0.5 }} />

              <div style={{
                padding: "12px 16px", cursor: "pointer",
                display: "flex", alignItems: "flex-start", gap: 14,
              }} onClick={() => setExpanded(isOpen ? null : d.id)}>
                {/* Severity + Priority */}
                <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 4, paddingTop: 2, flexShrink: 0 }}>
                  <Dot color={sc} size={8} pulse={d.severity === "critical"} />
                </div>

                {/* Content */}
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", marginBottom: 6 }}>
                    <RepoPill repoId={d.repo} />
                    {d.impactRepos && d.impactRepos.map(r => (
                      <span key={r} style={{ fontSize: 10, fontFamily: T.mono, color: T.t4, display: "flex", alignItems: "center", gap: 2 }}>
                        → <RepoPill repoId={r} />
                      </span>
                    ))}
                    <Badge color={sc} bg={`${sc}12`}>{d.severity}</Badge>
                    {d.tags.map(t => <Badge key={t} color={T.t3} bg={T.bg3}>{t}</Badge>)}
                  </div>
                  <div style={{ fontSize: 14, color: T.t1, fontFamily: T.sans, fontWeight: 500, lineHeight: 1.4, marginBottom: 4 }}>
                    {d.question}
                  </div>
                  <div style={{ fontSize: 12, color: T.t3, fontFamily: T.sans, display: "flex", alignItems: "center", gap: 8 }}>
                    <span style={{ fontFamily: T.mono, fontSize: 11 }}>{d.agentId}</span>
                    <span style={{ color: T.t4 }}>·</span>
                    <span>{d.agentGoal}</span>
                    <span style={{ color: T.t4 }}>·</span>
                    <span>{d.age}</span>
                    {d.assignedTo && (
                      <>
                        <span style={{ color: T.t4 }}>·</span>
                        <span>@{d.assignedTo}</span>
                      </>
                    )}
                  </div>
                </div>

                {/* Quick actions (always visible) */}
                <div style={{ display: "flex", gap: 6, flexShrink: 0, alignSelf: "center" }}>
                  <button style={{
                    background: T.okSolid, color: T.ok, border: `1px solid ${T.ok}25`,
                    borderRadius: 6, padding: "6px 14px", fontSize: 12, fontWeight: 500,
                    cursor: "pointer", fontFamily: T.sans,
                  }} onClick={e => e.stopPropagation()}>Approve</button>
                  <button style={{
                    background: T.errS, color: T.err, border: `1px solid ${T.err}20`,
                    borderRadius: 6, padding: "6px 14px", fontSize: 12, fontWeight: 500,
                    cursor: "pointer", fontFamily: T.sans,
                  }} onClick={e => e.stopPropagation()}>Deny</button>
                  <button style={{
                    background: "transparent", color: T.t3, border: `1px solid ${T.border}`,
                    borderRadius: 6, padding: "6px 12px", fontSize: 12,
                    cursor: "pointer", fontFamily: T.sans,
                  }} onClick={e => e.stopPropagation()}>Comment</button>
                </div>
              </div>

              {/* Expanded context */}
              {isOpen && (
                <div style={{
                  padding: "0 16px 14px 38px",
                  borderTop: `1px solid ${T.borderS}`, marginTop: 0, paddingTop: 12,
                }}>
                  {d.context.type === "diff" && (
                    <>
                      <div style={{ fontSize: 11, fontFamily: T.mono, color: T.t4, marginBottom: 6 }}>
                        {d.context.file}
                      </div>
                      <DiffBlock lines={d.context.lines} />
                    </>
                  )}
                  {d.context.type === "log" && (
                    <div style={{
                      background: T.bg0, borderRadius: 6, padding: "8px 12px",
                      fontFamily: T.mono, fontSize: 12, color: T.warn,
                      border: `1px solid ${T.borderS}`,
                    }}>{d.context.text}</div>
                  )}
                  {d.context.type === "screenshot" && (
                    <div style={{
                      background: T.bg0, borderRadius: 6, padding: "12px 14px",
                      fontFamily: T.sans, fontSize: 12, color: T.t3, fontStyle: "italic",
                      border: `1px solid ${T.borderS}`,
                    }}>{d.context.description}</div>
                  )}
                </div>
              )}
            </Card>
          );
        })}
      </div>
    </div>
  );
};

/* ═══════════════════════════════════════════
   FEATURE 2: SESSION REPLAY
   ═══════════════════════════════════════════ */
const SessionReplay = () => {
  const [replayTab, setReplayTab] = useState("diff");
  const session = sessions.find(s => s.id === "S-a82f");

  const repoForEvent = (repoId) => workspace.repos.find(r => r.id === repoId);
  const typeIcon = { system: "◦", read: "◎", edit: "✎", tool: "⚡", switch: "⇋", test: "▷", decision: "⬡" };

  const diffFiles = [
    { repo: "shared-protobufs", files: [
      { path: "proto/payment/v1/intent.proto", additions: 12, deletions: 0 },
      { path: "gen/rust/payment_intent.rs", additions: 34, deletions: 8 },
    ]},
    { repo: "payment-gateway", files: [
      { path: "src/services/payment.rs", additions: 18, deletions: 3 },
      { path: "src/handlers/checkout.rs", additions: 5, deletions: 1 },
    ]},
    { repo: "frontend-web", files: [
      { path: "src/types/payment.ts", additions: 8, deletions: 2 },
      { path: "src/components/CheckoutForm.tsx", additions: 22, deletions: 6 },
    ]},
  ];

  return (
    <div style={{ display: "flex", height: "100%", overflow: "hidden" }}>
      {/* Timeline sidebar */}
      <div style={{
        width: 300, flexShrink: 0, background: T.bg1,
        borderRight: `1px solid ${T.borderS}`, display: "flex", flexDirection: "column",
      }}>
        {/* Session header */}
        <div style={{ padding: "14px 14px 10px", borderBottom: `1px solid ${T.borderS}` }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
            <Dot color={statusColor(session.status)} size={8} pulse={session.status === "active"} />
            <span style={{ fontSize: 12, fontFamily: T.mono, color: T.t2 }}>{session.id}</span>
            <Badge color={T.purple} bg={T.purpleS}>Opus 4.6</Badge>
          </div>
          <div style={{ fontSize: 14, fontWeight: 500, color: T.t1, fontFamily: T.sans, lineHeight: 1.3, marginBottom: 8 }}>
            {session.goal}
          </div>
          <div style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
            {session.repos.map(r => <RepoPill key={r} repoId={r} />)}
          </div>
        </div>

        {/* Timeline events */}
        <div style={{ flex: 1, overflowY: "auto", padding: "8px 0" }}>
          <div style={{ padding: "0 14px 6px" }}>
            <SectionLabel>Timeline</SectionLabel>
          </div>
          {replayTimeline.map((evt, i) => {
            const repo = evt.repo ? repoForEvent(evt.repo) : null;
            const isSwitch = evt.type === "switch";
            return (
              <div key={i} style={{
                display: "flex", gap: 10, padding: "5px 14px",
                cursor: "pointer",
                background: evt.type === "decision" ? T.errS : "transparent",
              }}>
                {/* Timeline line */}
                <div style={{
                  display: "flex", flexDirection: "column", alignItems: "center",
                  width: 20, flexShrink: 0,
                }}>
                  <div style={{
                    width: isSwitch ? 16 : 6, height: isSwitch ? 16 : 6,
                    borderRadius: isSwitch ? 3 : "50%",
                    background: evt.type === "decision" ? T.err :
                      evt.type === "test" ? (evt.result === "pass" ? T.ok : T.err) :
                      repo ? repo.color : T.t4,
                    display: "flex", alignItems: "center", justifyContent: "center",
                    fontSize: 9, color: T.bg0, fontWeight: 700,
                    marginTop: 4,
                  }}>
                    {isSwitch && "⇋"}
                  </div>
                  {i < replayTimeline.length - 1 && (
                    <div style={{
                      width: 1, flex: 1, minHeight: 12,
                      background: isSwitch ? `${repo?.color || T.t4}40` : T.borderS,
                    }} />
                  )}
                </div>

                {/* Event content */}
                <div style={{ flex: 1, paddingBottom: 8 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    <span style={{ fontSize: 10, fontFamily: T.mono, color: T.t4 }}>{evt.time}</span>
                    {repo && (
                      <span style={{ fontSize: 10, fontFamily: T.mono, color: repo.color }}>
                        {repo.icon} {repo.label}
                      </span>
                    )}
                  </div>
                  <div style={{
                    fontSize: 12, color: evt.type === "decision" ? T.err : T.t2,
                    fontFamily: T.sans, marginTop: 2, lineHeight: 1.4,
                    fontWeight: isSwitch || evt.type === "decision" ? 500 : 400,
                  }}>
                    {evt.action}
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {/* Main pane */}
      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
        {/* Tab bar */}
        <div style={{
          display: "flex", alignItems: "center", borderBottom: `1px solid ${T.borderS}`,
          background: T.bg2, padding: "0 16px",
        }}>
          {[
            { key: "diff", label: "Unified Diff" },
            { key: "files", label: "File Tree" },
            { key: "log", label: "Structured Log" },
            { key: "terminal", label: "Terminal" },
          ].map(tab => (
            <button key={tab.key} onClick={() => setReplayTab(tab.key)} style={{
              background: "transparent", border: "none", cursor: "pointer",
              padding: "10px 14px", fontSize: 12, fontFamily: T.sans, fontWeight: 500,
              color: replayTab === tab.key ? T.t1 : T.t3,
              borderBottom: replayTab === tab.key ? `2px solid ${T.ember}` : "2px solid transparent",
            }}>{tab.label}</button>
          ))}
          <div style={{ flex: 1 }} />
          {/* Atomic merge CTA */}
          <button style={{
            background: `linear-gradient(135deg, ${T.ember}, ${T.molten})`,
            color: "#fff", border: "none", borderRadius: 6,
            padding: "6px 16px", fontSize: 12, fontWeight: 500,
            cursor: "pointer", fontFamily: T.sans,
            display: "flex", alignItems: "center", gap: 6,
          }}>
            <span style={{ fontSize: 14 }}>⊕</span> Atomic Merge → 3 PRs
          </button>
        </div>

        {/* Content area */}
        <div style={{ flex: 1, overflowY: "auto", padding: 20 }}>
          {replayTab === "diff" && (
            <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
              {diffFiles.map(group => {
                const repo = workspace.repos.find(r => r.id === group.repo);
                return (
                  <div key={group.repo}>
                    <div style={{
                      display: "flex", alignItems: "center", gap: 8, marginBottom: 10,
                      padding: "8px 12px", background: `${repo.color}08`,
                      borderRadius: 6, border: `1px solid ${repo.color}15`,
                    }}>
                      <span style={{ fontSize: 14, color: repo.color }}>{repo.icon}</span>
                      <span style={{ fontSize: 13, fontFamily: T.mono, fontWeight: 500, color: repo.color }}>
                        {repo.label}
                      </span>
                      <span style={{ fontSize: 11, fontFamily: T.mono, color: T.t4, marginLeft: "auto" }}>
                        {group.files.length} files changed
                      </span>
                    </div>
                    {group.files.map((f, fi) => (
                      <div key={fi} style={{
                        display: "flex", alignItems: "center", gap: 10,
                        padding: "8px 12px 8px 28px",
                        borderBottom: fi < group.files.length - 1 ? `1px solid ${T.borderS}` : "none",
                      }}>
                        <span style={{ fontSize: 12, fontFamily: T.mono, color: T.t2, flex: 1 }}>{f.path}</span>
                        <span style={{ fontSize: 11, fontFamily: T.data, fontWeight: 500, color: T.ok }}>+{f.additions}</span>
                        <span style={{ fontSize: 11, fontFamily: T.data, fontWeight: 500, color: T.err }}>-{f.deletions}</span>
                      </div>
                    ))}
                  </div>
                );
              })}
            </div>
          )}
          {replayTab === "files" && (
            <div style={{ fontFamily: T.mono, fontSize: 12 }}>
              {workspace.repos.filter(r => session.repos.includes(r.id)).map(repo => (
                <div key={repo.id} style={{ marginBottom: 16 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 6, color: repo.color, fontWeight: 500 }}>
                    <span>{repo.icon}</span> {repo.label}/
                  </div>
                  {diffFiles.find(d => d.repo === repo.id)?.files.map((f, i) => (
                    <div key={i} style={{
                      padding: "4px 8px 4px 24px", color: T.t2, cursor: "pointer",
                      borderRadius: 4,
                    }}>
                      <span style={{ color: T.t4, marginRight: 6 }}>📄</span>
                      {f.path}
                    </div>
                  ))}
                </div>
              ))}
            </div>
          )}
          {replayTab === "log" && (
            <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              {replayTimeline.filter(e => e.type !== "system").map((evt, i) => {
                const repo = evt.repo ? repoForEvent(evt.repo) : null;
                return (
                  <div key={i} style={{
                    display: "flex", alignItems: "center", gap: 10,
                    padding: "6px 10px", borderRadius: 4,
                    background: i % 2 === 0 ? T.bg2 : "transparent",
                  }}>
                    <span style={{ fontSize: 11, fontFamily: T.mono, color: T.t4, width: 36, flexShrink: 0 }}>{evt.time}</span>
                    <span style={{ fontSize: 12, width: 20, textAlign: "center", flexShrink: 0 }}>{typeIcon[evt.type]}</span>
                    {repo && <span style={{ fontSize: 10, fontFamily: T.mono, color: repo.color, width: 130, flexShrink: 0 }}>{repo.icon} {repo.label}</span>}
                    {!repo && <span style={{ width: 130, flexShrink: 0 }} />}
                    <span style={{
                      fontSize: 12, fontFamily: T.sans, color: T.t2, flex: 1,
                      fontWeight: evt.type === "switch" || evt.type === "decision" ? 500 : 400,
                    }}>{evt.action}</span>
                    {evt.result && (
                      <Badge color={evt.result === "pass" ? T.ok : T.err} bg={evt.result === "pass" ? T.okS : T.errS}>
                        {evt.result}
                      </Badge>
                    )}
                  </div>
                );
              })}
            </div>
          )}
          {replayTab === "terminal" && (
            <div style={{
              background: T.bg0, borderRadius: 6, border: `1px solid ${T.borderS}`,
              padding: 14, fontFamily: T.mono, fontSize: 12, color: T.t3, lineHeight: 1.7, minHeight: 300,
            }}>
              <div style={{ color: T.t4 }}>$ forgemux attach S-a82f</div>
              <div style={{ color: T.t4 }}>Connected to session S-a82f1c03 (Opus 4.6)</div>
              <div style={{ color: T.t4 }}>Workspace: Checkout Experience (3 repos)</div>
              <div>&nbsp;</div>
              <div><span style={{ color: T.ember }}>❯</span> Reading proto/payment/v1/intent.proto...</div>
              <div><span style={{ color: T.emberG }}>⚡ Read</span> 42 lines from shared-protobufs</div>
              <div><span style={{ color: T.ember }}>❯</span> Adding recurring_interval field</div>
              <div><span style={{ color: T.emberG }}>⚡ Edit</span> proto/payment/v1/intent.proto (+12 lines)</div>
              <div><span style={{ color: T.emberG }}>⚡ Bash</span> protoc --rust_out=gen/ proto/payment/v1/*.proto</div>
              <div style={{ color: T.ok }}>✓ Generated Rust bindings</div>
              <div>&nbsp;</div>
              <div style={{ color: T.purple }}>⇋ Context switch → payment-gateway</div>
              <div><span style={{ color: T.ember }}>❯</span> Updating PaymentService handler...</div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

/* ═══════════════════════════════════════════
   FEATURE 3: FLEET DASHBOARD
   ═══════════════════════════════════════════ */
const FleetDashboard = () => {
  const budget = workspace.attentionBudget;
  const budgetPct = (budget.used / budget.total) * 100;
  const budgetColor = budgetPct < 50 ? T.ok : budgetPct < 80 ? T.warn : T.err;

  const activeS = sessions.filter(s => s.status === "active");
  const blockedS = sessions.filter(s => s.status === "blocked");
  const queuedS = sessions.filter(s => s.status === "queued");
  const completeS = sessions.filter(s => s.status === "complete");
  const totalCost = sessions.reduce((s, a) => s + a.cost, 0);

  return (
    <div style={{ padding: 24 }}>
      {/* Attention Budget + Summary strip */}
      <div style={{ display: "flex", gap: 12, marginBottom: 20 }}>
        {/* Attention budget */}
        <Card style={{ flex: "0 0 280px", padding: 16 }}>
          <SectionLabel>Team Attention Budget</SectionLabel>
          <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 10 }}>
            <div style={{
              width: 56, height: 56, borderRadius: "50%", position: "relative",
              background: `conic-gradient(${budgetColor} ${budgetPct}%, ${T.bg4} ${budgetPct}%)`,
              display: "flex", alignItems: "center", justifyContent: "center",
            }}>
              <div style={{
                width: 44, height: 44, borderRadius: "50%", background: T.bg1,
                display: "flex", alignItems: "center", justifyContent: "center",
                fontSize: 15, fontFamily: T.data, fontWeight: 300, color: budgetColor, letterSpacing: "-0.02em",
              }}>
                {budget.used}/{budget.total}
              </div>
            </div>
            <div>
              <div style={{ fontSize: 13, color: T.t1, fontFamily: T.sans, fontWeight: 500 }}>
                {budget.total - budget.used} decisions remaining
              </div>
              <div style={{ fontSize: 12, color: T.t3, fontFamily: T.sans }}>
                {budget.used} resolved today
              </div>
            </div>
          </div>
          <div style={{ fontSize: 11, color: T.t4, fontFamily: T.sans, fontStyle: "italic" }}>
            Budget resets at midnight. Unresolved decisions pause agents.
          </div>
        </Card>

        {/* Stats */}
        {[
          { label: "Active", value: activeS.length, color: T.ok },
          { label: "Blocked", value: blockedS.length, color: T.err },
          { label: "Queued", value: queuedS.length, color: T.t2 },
          { label: "Complete", value: completeS.length, color: T.info },
          { label: "Cost today", value: `$${totalCost.toFixed(2)}`, color: T.molten },
        ].map((s, i) => (
          <Card key={i} style={{ flex: 1, padding: "14px 16px" }}>
            <div style={{ fontSize: 26, fontWeight: 300, fontFamily: T.data, color: s.color, lineHeight: 1, letterSpacing: "-0.03em" }}>{s.value}</div>
            <div style={{ fontSize: 11, color: T.t3, fontFamily: T.sans, marginTop: 6 }}>{s.label}</div>
          </Card>
        ))}
      </div>

      {/* Session fleet grid */}
      <SectionLabel>Active & Blocked Sessions — Risk Heatmap</SectionLabel>
      <div style={{ display: "flex", flexDirection: "column", gap: 6, marginBottom: 24 }}>
        {[...activeS, ...blockedS].map((s) => {
          const rc = riskColor(s.risk);
          return (
            <Card key={s.id} style={{
              overflow: "hidden",
              borderLeft: `3px solid ${rc}`,
            }}>
              <div style={{ padding: "12px 16px", display: "flex", alignItems: "center", gap: 14 }}>
                {/* Risk indicator */}
                <div style={{
                  width: 36, height: 36, borderRadius: 6,
                  background: `${rc}12`, border: `1px solid ${rc}20`,
                  display: "flex", alignItems: "center", justifyContent: "center",
                  flexShrink: 0,
                }}>
                  <Dot color={rc} size={10} pulse={s.risk === "red"} />
                </div>

                {/* Info */}
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4, flexWrap: "wrap" }}>
                    <span style={{ fontSize: 14, fontWeight: 500, color: T.t1, fontFamily: T.sans }}>{s.goal}</span>
                    <Badge color={statusColor(s.status)} bg={`${statusColor(s.status)}12`}>{s.status}</Badge>
                    {s.pendingDecisions > 0 && (
                      <Badge color={T.molten} bg={T.warnS}>{s.pendingDecisions} decision{s.pendingDecisions > 1 ? "s" : ""}</Badge>
                    )}
                  </div>
                  <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                    {s.repos.map(r => <RepoPill key={r} repoId={r} />)}
                  </div>
                </div>

                {/* Metrics */}
                <div style={{ display: "flex", alignItems: "center", gap: 16, flexShrink: 0 }}>
                  {/* Context */}
                  <div style={{ width: 80, display: "flex", flexDirection: "column", gap: 3 }}>
                    <div style={{ display: "flex", justifyContent: "space-between" }}>
                      <span style={{ fontSize: 9, fontFamily: T.mono, color: T.t4 }}>CTX</span>
                      <span style={{ fontSize: 10, fontFamily: T.data, color: contextColor(s.context), fontWeight: 500 }}>{s.context}%</span>
                    </div>
                    <MiniBar value={s.context} color={contextColor(s.context)} h={3} />
                  </div>
                  {/* Tests */}
                  <div style={{ width: 50 }}>
                    <Badge
                      color={s.testsStatus === "passing" ? T.ok : s.testsStatus === "failing" ? T.err : T.t3}
                      bg={s.testsStatus === "passing" ? T.okS : s.testsStatus === "failing" ? T.errS : T.bg3}
                    >
                      {s.testsStatus === "passing" ? "✓ Tests" : s.testsStatus === "failing" ? "✗ Tests" : "…"}
                    </Badge>
                  </div>
                  {/* Stats */}
                  <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 2 }}>
                    <span style={{ fontSize: 11, fontFamily: T.data, fontWeight: 400, color: T.t3 }}>{s.tokens} tok</span>
                    <span style={{ fontSize: 11, fontFamily: T.data, fontWeight: 400, color: T.t4 }}>${s.cost.toFixed(2)} · {s.uptime}</span>
                  </div>
                </div>
              </div>
            </Card>
          );
        })}
      </div>

      {/* Queued & completed */}
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}>
        <div>
          <SectionLabel>Queued ({queuedS.length})</SectionLabel>
          <Card style={{ padding: 0 }}>
            {queuedS.map((s, i) => (
              <div key={s.id} style={{
                padding: "10px 14px", display: "flex", alignItems: "center", gap: 10,
                borderBottom: i < queuedS.length - 1 ? `1px solid ${T.borderS}` : "none",
              }}>
                <Dot color={T.t4} size={6} />
                <div style={{ flex: 1 }}>
                  <div style={{ fontSize: 13, color: T.t2, fontFamily: T.sans }}>{s.goal}</div>
                  <div style={{ display: "flex", gap: 4, marginTop: 4 }}>
                    {s.repos.map(r => <RepoPill key={r} repoId={r} />)}
                  </div>
                </div>
                <span style={{ fontSize: 11, fontFamily: T.mono, color: T.t4 }}>{s.model}</span>
              </div>
            ))}
          </Card>
        </div>
        <div>
          <SectionLabel>Recently Completed</SectionLabel>
          <Card style={{ padding: 0 }}>
            {completeS.map((s, i) => (
              <div key={s.id} style={{
                padding: "10px 14px", display: "flex", alignItems: "center", gap: 10,
                borderBottom: i < completeS.length - 1 ? `1px solid ${T.borderS}` : "none",
              }}>
                <Dot color={T.info} size={6} />
                <div style={{ flex: 1 }}>
                  <div style={{ fontSize: 13, color: T.t2, fontFamily: T.sans }}>{s.goal}</div>
                  <div style={{ display: "flex", gap: 4, marginTop: 4 }}>
                    {s.repos.map(r => <RepoPill key={r} repoId={r} />)}
                  </div>
                </div>
                <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 2 }}>
                  <span style={{ fontSize: 11, fontFamily: T.data, fontWeight: 500, color: T.ok }}>+{s.linesAdded} <span style={{ color: T.err }}>-{s.linesRemoved}</span></span>
                  <span style={{ fontSize: 11, fontFamily: T.data, fontWeight: 400, color: T.t4 }}>${s.cost.toFixed(2)} · {s.uptime}</span>
                </div>
              </div>
            ))}
          </Card>
        </div>
      </div>
    </div>
  );
};

/* ═══════════════════════════════════════════
   MAIN SHELL
   ═══════════════════════════════════════════ */
export default function ForgemuxHub() {
  const [view, setView] = useState("fleet");

  const views = [
    { key: "fleet", label: "Dashboard" },
    { key: "queue", label: "Decisions" },
    { key: "replay", label: "Session Replay" },
  ];

  const pendingCount = decisions.length;

  return (
    <div style={{
      width: "100%", height: "100vh", background: T.bg0, color: T.t1,
      fontFamily: T.sans, display: "flex", flexDirection: "column", overflow: "hidden",
    }}>
      <link href="https://fonts.googleapis.com/css2?family=Poppins:wght@300;400;500;600&family=Outfit:wght@300;400;500;600&family=JetBrains+Mono:wght@400;500;600&display=swap" rel="stylesheet" />

      {/* Top nav */}
      <div style={{
        display: "flex", alignItems: "center", borderBottom: `1px solid ${T.borderS}`,
        background: T.bg1, padding: "0 20px", flexShrink: 0, height: 46,
      }}>
        {/* Logo */}
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginRight: 24 }}>
          <div style={{
            width: 22, height: 22, borderRadius: 5,
            background: `linear-gradient(135deg, ${T.ember} 0%, ${T.molten} 100%)`,
            display: "flex", alignItems: "center", justifyContent: "center",
          }}>
            <svg width="12" height="12" viewBox="0 0 14 14" fill="none">
              <path d="M2 4L7 2L12 4V10L7 12L2 10V4Z" stroke="white" strokeWidth="1.5" strokeLinejoin="round" />
              <path d="M7 2V12" stroke="white" strokeWidth="1" opacity="0.5" />
              <path d="M2 4L7 6L12 4" stroke="white" strokeWidth="1" opacity="0.5" />
            </svg>
          </div>
          <span style={{ fontSize: 14, fontWeight: 600, letterSpacing: "-0.02em" }}>forgemux</span>
        </div>

        {/* Org / Workspace breadcrumb */}
        <div style={{ display: "flex", alignItems: "center", gap: 6, marginRight: 24 }}>
          <span style={{ fontSize: 12, color: T.t3, fontFamily: T.sans }}>{workspace.org}</span>
          <span style={{ color: T.t4, fontSize: 10 }}>/</span>
          <span style={{
            fontSize: 12, color: T.t1, fontWeight: 500, fontFamily: T.sans,
            padding: "3px 8px", background: T.bg3, borderRadius: 4, cursor: "pointer",
          }}>{workspace.name}</span>
        </div>

        {/* Nav tabs */}
        {views.map((v) => (
          <button key={v.key} onClick={() => setView(v.key)} style={{
            background: "transparent", border: "none", cursor: "pointer",
            padding: "0 14px", height: 46, display: "flex", alignItems: "center", gap: 6,
            borderBottom: view === v.key ? `2px solid ${T.ember}` : "2px solid transparent",
            transition: "all 0.12s ease",
          }}>
            <span style={{ fontSize: 13, fontWeight: 500, fontFamily: T.sans, color: view === v.key ? T.t1 : T.t3 }}>
              {v.label}
            </span>
            {v.key === "queue" && pendingCount > 0 && (
              <span style={{
                fontSize: 10, fontFamily: T.data, fontWeight: 600, color: "#fff",
                background: T.ember, borderRadius: 8, padding: "1px 6px", lineHeight: "14px",
              }}>{pendingCount}</span>
            )}
          </button>
        ))}

        <div style={{ flex: 1 }} />

        {/* Repo indicators */}
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginRight: 16 }}>
          {workspace.repos.map(r => (
            <div key={r.id} style={{
              display: "flex", alignItems: "center", gap: 4,
              fontSize: 10, fontFamily: T.mono, color: r.color, opacity: 0.7,
            }}>
              <span>{r.icon}</span>
            </div>
          ))}
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 5 }}>
          <Dot color={T.ok} size={6} />
          <span style={{ fontSize: 11, fontFamily: T.mono, color: T.ok }}>live</span>
        </div>
      </div>

      {/* Content */}
      <div style={{ flex: 1, overflow: "auto" }}>
        {view === "fleet" && <FleetDashboard />}
        {view === "queue" && <DecisionQueue />}
        {view === "replay" && <SessionReplay />}
      </div>
    </div>
  );
}
