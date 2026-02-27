# Forgemux Hub - Technology Design Document

**Version:** 3.0  
**Date:** February 27, 2026  
**Status:** Design Draft  

---

## 1. Executive Summary

This document defines the technical architecture for the Forgemux Hub Dashboard, a React-based web application for monitoring and managing AI agent sessions across multi-repo workspaces. The design prioritizes real-time updates, high data density, and cross-repo session visibility.

**Key Technical Decisions:**
- React 18+ with functional components and hooks
- Server-Sent Events (SSE) for real-time updates
- CSS-in-JS for component styling (no external CSS files)
- Three-font typography system for information hierarchy
- Dark mode as default (light mode non-goal for v1)

---

## 2. System Architecture

### 2.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Client Browser                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │   React App  │  │  SSE Client  │  │  WebSocket       │  │
│  │              │  │  (Session    │  │  (Terminal       │  │
│  │              │  │   Updates)   │  │   Attach)        │  │
│  └──────────────┘  └──────────────┘  └──────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      Forgemux Hub                           │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │  REST API    │  │  SSE Stream  │  │  WS Relay        │  │
│  │  (CRUD)      │  │  Endpoint    │  │  (tmux bridge)   │  │
│  └──────────────┘  └──────────────┘  └──────────────────┘  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │  Session     │  │  Decision    │  │  Usage           │  │
│  │  Aggregator  │  │  Queue       │  │  Collector       │  │
│  └──────────────┘  └──────────────┘  └──────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    Edge Nodes (forged)                      │
│         Session State • Transcripts • Agent Logs            │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 Communication Patterns

**Real-Time Updates (SSE):**
- Single SSE connection per client
- Server pushes session state changes, decision events, usage updates
- Events: `session.update`, `decision.new`, `decision.resolved`, `usage.tick`
- Reconnection with `Last-Event-ID` for missed events

**Terminal Attach (WebSocket):**
- WebSocket for bidirectional terminal I/O
- Hub acts as relay between browser and edge node
- Protocol: RESUME/EVENT/INPUT/ACK (from Forgemux spec)

**REST API:**
- Initial data load
- Decision actions (approve/deny/comment)
- Session lifecycle (start/stop/pause)
- Historical queries (session replay)

---

## 3. Frontend Architecture

### 3.1 Technology Stack

| Layer | Technology | Version | Rationale |
|-------|-----------|---------|-----------|
| Framework | React | 18.x | Component model, hooks, concurrent features |
| Language | TypeScript | 5.x | Type safety, IDE support |
| Styling | CSS-in-JS (inline) | - | No build step, dynamic theming |
| Fonts | Google Fonts | - | Poppins, Outfit, JetBrains Mono |
| State | React Context + Hooks | - | Lightweight, no Redux needed |
| Icons | SVG (inline) | - | No icon font dependency |
| Build | Vite | 5.x | Fast dev server, optimized builds |

### 3.2 Design System (Theme Object)

```typescript
// T.ts - Central theme definition
export const Theme = {
  // Backgrounds (5 levels of depth)
  bg0: "#0A0A0C",  // Deepest (terminal)
  bg1: "#101013",  // Cards, sidebars
  bg2: "#161619",  // Elevated surfaces
  bg3: "#1E1E23",  // Inputs, buttons
  bg4: "#26262D",  // Hover states
  
  // Borders
  border: "#28282F",
  borderS: "#1C1C22",
  
  // Text
  t1: "#E4E2DF",   // Primary
  t2: "#98968F",   // Secondary
  t3: "#5A5955",   // Tertiary
  t4: "#3A3937",   // Disabled
  
  // Semantic Colors
  ember: "#E8622C",      // Primary brand
  emberG: "#F0884D",     // Ember glow
  emberS: "rgba(232,98,44,0.08)",
  molten: "#FF9F43",     // Cost, high severity
  ok: "#34D399",         // Success
  okD: "#22C55E",
  okS: "rgba(52,211,153,0.08)",
  okSolid: "rgba(52,211,153,0.15)",
  warn: "#FBBF24",       // Warning, idle
  warnS: "rgba(251,191,36,0.08)",
  err: "#F87171",        // Error, blocked
  errD: "#EF4444",
  errS: "rgba(248,113,113,0.08)",
  info: "#60A5FA",       // Info, complete
  infoS: "rgba(96,165,250,0.08)",
  purple: "#A78BFA",     // Merged, Opus
  purpleS: "rgba(167,139,250,0.08)",
  cyan: "#22D3EE",
  cyanS: "rgba(34,211,238,0.08)",
  
  // Typography
  mono: "'JetBrains Mono', monospace",
  sans: "'Poppins', sans-serif",
  data: "'Outfit', sans-serif",
} as const;

// Type exports for TypeScript
export type Theme = typeof Theme;
```

### 3.3 Typography Rules

| Token | Font | Weights | Usage |
|-------|------|---------|-------|
| `T.sans` | Poppins | 300–500 | Headers (500), body text (400), labels, buttons, nav |
| `T.data` | Outfit | 300–500 | Numbers, costs, token counts, percentages, diffs |
| `T.mono` | JetBrains Mono | 400–600 | Session IDs, branch names, file paths, terminal, code |

**Rules:**
- Numbers/metrics humans scan → `T.data` (Outfit)
- Words humans read → `T.sans` (Poppins)
- Code/identifiers/paths → `T.mono` (JetBrains Mono)
- Max weight 500 for Poppins/Outfit; only `T.mono` uses 600

### 3.4 Component Hierarchy

```
ForgemuxHub (App Shell)
├── TopNavigation
│   ├── Logo
│   ├── Breadcrumb (Org / Workspace)
│   ├── NavTabs (Dashboard | Decisions | Session Replay)
│   ├── RepoIndicators
│   └── LiveIndicator
│
├── FleetDashboard
│   ├── AttentionBudgetMeter
│   ├── SessionSummaryStats
│   ├── SessionRiskHeatmap
│   │   └── SessionCard (risk-colored)
│   ├── QueuedPanel
│   └── CompletedPanel
│
├── DecisionQueue
│   ├── FilterBar (repo filters)
│   └── DecisionCard
│       ├── SeverityStripe
│       ├── RepoPills
│       ├── ImpactChain (cross-repo)
│       ├── QuickActions (Approve/Deny/Comment)
│       └── ExpandedContext (diff/log/screenshot)
│
└── SessionReplay
    ├── TimelineSidebar
    │   └── TimelineEvent
    ├── TabBar (Diff | Files | Log | Terminal)
    ├── UnifiedDiffView
    ├── FileTreeView
    ├── StructuredLogView
    ├── TerminalView
    └── AtomicMergeCTA
```

### 3.5 Shared Components

```typescript
// Dot.tsx - Status indicator with optional pulse
interface DotProps {
  color: string;
  size?: number;
  pulse?: boolean;
}

// Badge.tsx - Semantic label
interface BadgeProps {
  children: React.ReactNode;
  color?: string;
  bg?: string;
  style?: React.CSSProperties;
}

// RepoPill.tsx - Repository identifier with icon
interface RepoPillProps {
  repoId: string;
}

// MiniBar.tsx - Progress/health bar
interface MiniBarProps {
  value: number;
  max?: number;
  color?: string;
  h?: number;
}

// Card.tsx - Container with consistent styling
interface CardProps {
  children: React.ReactNode;
  style?: React.CSSProperties;
}

// SectionLabel.tsx - Uppercase section headers
interface SectionLabelProps {
  children: React.ReactNode;
}

// DiffBlock.tsx - Syntax-highlighted diff
interface DiffBlockProps {
  lines: Array<{
    type: 'ctx' | 'add' | 'del';
    text: string;
  }>;
}
```

---

## 4. Data Models

### 4.1 Core Types

```typescript
// Workspace.ts
interface Workspace {
  id: string;
  name: string;
  org: string;
  repos: Repository[];
  members: string[];
  attentionBudget: {
    used: number;
    total: number;
    resetAt: string; // ISO timestamp
  };
  settings: WorkspaceSettings;
}

interface Repository {
  id: string;
  label: string;
  color: string;
  icon: string;
  gitUrl: string;
  defaultBranch: string;
}

// Session.ts
interface Session {
  id: string;
  goal: string;
  status: 'active' | 'blocked' | 'queued' | 'complete' | 'error';
  risk: 'green' | 'yellow' | 'red';
  model: string; // "Sonnet 4.5", "Opus 4.6"
  repos: string[]; // Repository IDs
  
  // Context & Usage
  context: number; // 0-100%
  tokens: string; // Human-readable "52.7k"
  cost: number; // USD
  
  // Time
  uptime: string; // Human-readable "1h 23m"
  createdAt: string;
  completedAt?: string;
  
  // Work
  commits: number;
  linesAdded: number;
  linesRemoved: number;
  pendingDecisions: number;
  testsStatus: 'passing' | 'failing' | 'pending' | 'none';
  
  // Relations
  agentId: string;
  edgeNodeId: string;
}

// Decision.ts
interface Decision {
  id: string;
  agentId: string;
  agentGoal: string;
  repo: string; // Primary repo
  impactRepos?: string[]; // Cross-repo impact chain
  
  question: string;
  context: DecisionContext;
  
  severity: 'critical' | 'high' | 'medium' | 'low';
  tags: string[];
  
  // Assignment
  assignedTo: string | null;
  createdAt: string;
  age: string; // Human-readable "3m", "1h"
  
  // Resolution
  status: 'pending' | 'approved' | 'denied' | 'commented';
  resolvedAt?: string;
  resolvedBy?: string;
  comment?: string;
}

type DecisionContext = 
  | { type: 'diff'; file: string; lines: DiffLine[] }
  | { type: 'log'; text: string }
  | { type: 'screenshot'; description: string; url?: string };

interface DiffLine {
  type: 'ctx' | 'add' | 'del';
  text: string;
}

// TimelineEvent.ts (Session Replay)
interface TimelineEvent {
  id: string;
  sessionId: string;
  timestamp: string; // ISO
  time: string; // Display "0:02"
  type: 'system' | 'read' | 'edit' | 'tool' | 'switch' | 'test' | 'decision';
  repo: string | null;
  action: string;
  result?: 'pass' | 'fail';
  metadata?: Record<string, unknown>;
}

// DiffFile.ts
interface DiffFile {
  repo: string;
  files: Array<{
    path: string;
    additions: number;
    deletions: number;
    diff?: string; // Full diff content
  }>;
}
```

### 4.2 API Endpoints

```typescript
// GET /api/workspaces/:id
// Returns: Workspace

// GET /api/workspaces/:id/sessions
// Query: ?status=active&risk=red,yellow&repo=payment-gateway
// Returns: Session[]

// GET /api/workspaces/:id/decisions
// Query: ?status=pending&severity=critical,high&repo=all
// Returns: Decision[]

// POST /api/decisions/:id/approve
// Body: { comment?: string }
// Returns: Decision

// POST /api/decisions/:id/deny
// Body: { comment?: string }
// Returns: Decision

// POST /api/decisions/:id/comment
// Body: { comment: string }
// Returns: Decision

// GET /api/sessions/:id/timeline
// Returns: TimelineEvent[]

// GET /api/sessions/:id/diff
// Returns: DiffFile[]

// POST /api/sessions/:id/merge
// Body: { targetBranch: string }
// Returns: { prs: Array<{ repo: string; url: string }> }

// SSE /api/events
// Headers: Last-Event-ID: <last-id>
// Events: session.update, decision.new, decision.resolved, usage.tick
```

---

## 5. State Management

### 5.1 React Context Structure

```typescript
// contexts/WorkspaceContext.tsx
interface WorkspaceContextType {
  workspace: Workspace | null;
  loading: boolean;
  error: Error | null;
  refresh: () => Promise<void>;
}

// contexts/SessionContext.tsx
interface SessionContextType {
  sessions: Session[];
  activeSessions: Session[];
  blockedSessions: Session[];
  queuedSessions: Session[];
  completedSessions: Session[];
  totalCost: number;
  updateSession: (session: Session) => void;
}

// contexts/DecisionContext.tsx
interface DecisionContextType {
  decisions: Decision[];
  pendingDecisions: Decision[];
  criticalCount: number;
  filterRepo: string | 'all';
  setFilterRepo: (repo: string | 'all') => void;
  approve: (id: string, comment?: string) => Promise<void>;
  deny: (id: string, comment?: string) => Promise<void>;
  comment: (id: string, text: string) => Promise<void>;
}

// contexts/RealtimeContext.tsx
interface RealtimeContextType {
  connected: boolean;
  lastEventId: string | null;
  reconnect: () => void;
}
```

### 5.2 SSE Event Handling

```typescript
// hooks/useRealtime.ts
export function useRealtime() {
  const [connected, setConnected] = useState(false);
  const [lastEventId, setLastEventId] = useState<string | null>(null);
  
  useEffect(() => {
    const eventSource = new EventSource('/api/events', {
      headers: lastEventId ? { 'Last-Event-ID': lastEventId } : undefined
    });
    
    eventSource.onopen = () => setConnected(true);
    eventSource.onerror = () => setConnected(false);
    
    eventSource.addEventListener('session.update', (e) => {
      const session = JSON.parse(e.data);
      setLastEventId(e.lastEventId);
      updateSession(session);
    });
    
    eventSource.addEventListener('decision.new', (e) => {
      const decision = JSON.parse(e.data);
      setLastEventId(e.lastEventId);
      addDecision(decision);
    });
    
    eventSource.addEventListener('decision.resolved', (e) => {
      const decision = JSON.parse(e.data);
      setLastEventId(e.lastEventId);
      updateDecision(decision);
    });
    
    return () => eventSource.close();
  }, []);
  
  return { connected, lastEventId, reconnect };
}
```

---

## 6. Risk Scoring Algorithm

### 6.1 Session Risk Calculation

```typescript
function calculateRisk(session: Session): 'green' | 'yellow' | 'red' {
  // Red conditions (any one triggers red)
  if (session.context > 85) return 'red';
  if (session.status === 'blocked') return 'red';
  
  // Check for looping (would need history)
  // if (consecutiveFailedToolCalls > 3) return 'red';
  
  // Check for stale decisions
  // if (hasDecisionOlderThan(session, 15 * 60 * 1000)) return 'red';
  
  // Yellow conditions (any one triggers yellow)
  if (session.context >= 70) return 'yellow';
  if (session.testsStatus === 'failing') return 'yellow';
  // if (hasDecisionBetween(session, 5 * 60 * 1000, 15 * 60 * 1000)) return 'yellow';
  
  return 'green';
}

// Color mapping
const riskColor = {
  green: Theme.ok,
  yellow: Theme.warn,
  red: Theme.err
};
```

### 6.2 Context Health Color

```typescript
function contextColor(value: number): string {
  if (value < 40) return Theme.ok;      // Healthy
  if (value < 70) return Theme.molten;  // Elevated
  if (value < 85) return Theme.warn;    // Warning
  return Theme.err;                     // Critical
}
```

---

## 7. Session Replay Views

### 7.1 Timeline Sidebar

**Features:**
- Vertical timeline with connector lines
- Color-coded event markers
- Context switches highlighted with larger markers and colored lines
- Decision events with red background
- Click to jump to corresponding change

**Event Types & Icons:**
- System: `◦`
- Read: `◎`
- Edit: `✎`
- Tool: `⚡`
- Switch: `⇋`
- Test: `▷`
- Decision: `⬡`

### 7.2 Unified Diff View

**Features:**
- Files grouped by repository
- Repo headers with icon, color, and file count
- File rows show: path, additions (green), deletions (red)
- Click to expand full diff (future)

### 7.3 File Tree View

**Features:**
- Tree showing only repos session modified
- Modified files highlighted
- Click file to open diff

### 7.4 Structured Log View

**Table Columns:**
- Timestamp
- Event type icon
- Repository (with icon)
- Action description
- Result badge (pass/fail)

**Styling:**
- Alternating row backgrounds
- Context switches and decisions emphasized

### 7.5 Terminal View

**Features:**
- Monospaced dark background
- Color-coded output:
  - User prompts: ember
  - Tool calls: ember glow
  - Success: green
  - Errors: red
  - Context switches: purple
  - System info: gray

---

## 8. Performance Considerations

### 8.1 Virtualization

- Session lists > 50 items: Use `react-window` or similar
- Timeline > 100 events: Virtualize scroll
- Diff view: Lazy load full diff content

### 8.2 Memoization

```typescript
// Memoize expensive calculations
const filteredDecisions = useMemo(() => {
  if (filterRepo === 'all') return decisions;
  return decisions.filter(d => d.repo === filterRepo);
}, [decisions, filterRepo]);

// Memoize components
const SessionCard = React.memo(({ session }: { session: Session }) => {
  // ...
});
```

### 8.3 Debouncing

```typescript
// Debounce search/filter inputs
const [searchTerm, setSearchTerm] = useState('');
const debouncedSearch = useDebounce(searchTerm, 300);

useEffect(() => {
  // Perform search with debouncedSearch
}, [debouncedSearch]);
```

### 8.4 Image Optimization

- No external images in v1 (placeholders only)
- Future: Use WebP format with lazy loading

---

## 9. Security Considerations

### 9.1 Authentication

- JWT tokens stored in httpOnly cookies
- Token refresh via /api/auth/refresh
- Session expiry: 24 hours

### 9.2 Authorization

- Workspace-level access control
- Role-based: viewer, member, admin
- Decision assignment restricts approve/deny actions

### 9.3 XSS Prevention

- All user input escaped in JSX
- DangerouslySetInnerHTML only for trusted diff content
- Content Security Policy headers

### 9.4 CSRF Protection

- SameSite cookies
- CSRF tokens for state-changing POSTs

---

## 10. Accessibility

### 10.1 ARIA Labels

```tsx
<button 
  aria-label="Approve decision"
  onClick={handleApprove}
>
  Approve
</button>
```

### 10.2 Keyboard Navigation

- Tab order follows visual order
- Enter/Space to activate buttons
- Escape to close expanded decision cards
- Future: `a`/`d`/`c` shortcuts for decision actions

### 10.3 Color Contrast

- All text meets WCAG AA (4.5:1 ratio)
- Risk colors distinguishable without color (icons, patterns)

---

## 11. Error Handling

### 11.1 Error Boundaries

```tsx
class DashboardErrorBoundary extends React.Component {
  state = { hasError: false };
  
  static getDerivedStateFromError() {
    return { hasError: true };
  }
  
  render() {
    if (this.state.hasError) {
      return <ErrorFallback />;
    }
    return this.props.children;
  }
}
```

### 11.2 API Error Handling

```typescript
// Centralized error handling
async function apiRequest<T>(url: string, options?: RequestInit): Promise<T> {
  try {
    const response = await fetch(url, options);
    if (!response.ok) {
      throw new ApiError(response.status, await response.text());
    }
    return response.json();
  } catch (error) {
    // Log to error tracking service
    console.error('API Error:', error);
    throw error;
  }
}
```

---

## 12. Future Enhancements

### 12.1 Phase 2 Features

- **Keyboard shortcuts**: `1/2/3` for nav, `a/d/c` for decisions
- **Global search**: Cross-session transcript search
- **Filters**: Advanced filtering on all lists
- **Export**: Session reports as PDF/Markdown

### 12.2 Phase 3 Features

- **WebSocket terminal**: Full terminal attach in browser
- **Split view**: Side-by-side session comparison
- **Mobile responsive**: Basic mobile support
- **Notifications**: Browser push notifications

### 12.3 Phase 4 Features

- **Light mode**: Alternative color scheme
- **Custom themes**: User-defined color palettes
- **Plugins**: Extension system for custom views
- **Analytics**: Dashboard usage metrics

---

## 13. Testing Strategy

### 13.1 Unit Tests

```typescript
// Risk calculation
describe('calculateRisk', () => {
  it('returns red for context > 85%', () => {
    expect(calculateRisk({ context: 90, status: 'active' })).toBe('red');
  });
  
  it('returns green for healthy sessions', () => {
    expect(calculateRisk({ context: 50, status: 'active', testsStatus: 'passing' })).toBe('green');
  });
});
```

### 13.2 Component Tests

```typescript
// DecisionQueue.test.tsx
describe('DecisionQueue', () => {
  it('filters by repo', () => {
    render(<DecisionQueue />);
    fireEvent.click(screen.getByText('payment-gateway'));
    expect(screen.getAllByTestId('decision-card')).toHaveLength(2);
  });
});
```

### 13.3 E2E Tests

```typescript
// e2e/decisions.spec.ts
test('user can approve a decision', async ({ page }) => {
  await page.goto('/workspace/checkout-experience/decisions');
  await page.click('[data-testid="decision-D-0041"]');
  await page.click('text=Approve');
  await expect(page.locator('[data-testid="success-toast"]')).toBeVisible();
});
```

---

## 14. Deployment

### 14.1 Build Configuration

```typescript
// vite.config.ts
export default {
  build: {
    outDir: 'dist',
    sourcemap: true,
    rollupOptions: {
      output: {
        manualChunks: {
          vendor: ['react', 'react-dom'],
        },
      },
    },
  },
  server: {
    proxy: {
      '/api': 'http://localhost:8080',
      '/events': {
        target: 'http://localhost:8080',
        ws: true,
      },
    },
  },
};
```

### 14.2 Environment Variables

```bash
# .env.production
VITE_API_URL=https://hub.forgemux.io/api
VITE_WS_URL=wss://hub.forgemux.io/events
VITE_VERSION=$npm_package_version
```

---

## 15. File Structure

```
src/
├── components/
│   ├── ui/
│   │   ├── Dot.tsx
│   │   ├── Badge.tsx
│   │   ├── RepoPill.tsx
│   │   ├── MiniBar.tsx
│   │   ├── Card.tsx
│   │   ├── SectionLabel.tsx
│   │   └── DiffBlock.tsx
│   ├── layout/
│   │   ├── TopNavigation.tsx
│   │   └── MainLayout.tsx
│   ├── dashboard/
│   │   ├── FleetDashboard.tsx
│   │   ├── AttentionBudgetMeter.tsx
│   │   ├── SessionSummaryStats.tsx
│   │   ├── SessionRiskHeatmap.tsx
│   │   └── SessionCard.tsx
│   ├── decisions/
│   │   ├── DecisionQueue.tsx
│   │   ├── DecisionCard.tsx
│   │   └── FilterBar.tsx
│   └── replay/
│       ├── SessionReplay.tsx
│       ├── TimelineSidebar.tsx
│       ├── UnifiedDiffView.tsx
│       ├── FileTreeView.tsx
│       ├── StructuredLogView.tsx
│       └── TerminalView.tsx
├── contexts/
│   ├── WorkspaceContext.tsx
│   ├── SessionContext.tsx
│   ├── DecisionContext.tsx
│   └── RealtimeContext.tsx
├── hooks/
│   ├── useRealtime.ts
│   ├── useWorkspace.ts
│   ├── useSessions.ts
│   └── useDecisions.ts
├── lib/
│   ├── api.ts
│   ├── theme.ts
│   └── utils.ts
├── types/
│   ├── workspace.ts
│   ├── session.ts
│   ├── decision.ts
│   └── timeline.ts
├── App.tsx
└── main.tsx
```

---

## 16. Summary

This technology design provides a complete blueprint for implementing the Forgemux Hub Dashboard. Key highlights:

1. **Performance-first**: CSS-in-JS eliminates build steps, SSE provides efficient real-time updates
2. **Type-safe**: Full TypeScript coverage with strict typing
3. **Accessible**: WCAG AA compliance, keyboard navigation support
4. **Maintainable**: Clear component hierarchy, shared design system
5. **Extensible**: Context-based state management allows easy feature additions

The architecture balances immediate delivery needs (v1) with future scalability (multi-workspace, advanced features).
