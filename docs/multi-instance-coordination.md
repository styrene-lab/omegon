+++
id = "39e89189-5745-47fa-80ce-37a60f7ab1cc"
kind = "document"
title = "Multi-instance Omegon coordination — parallel work streams on the same repo"
status = "decided"
tags = []
aliases = ["multi-instance-coordination"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "directive-branch-lifecycle"
related = ["a2a-protocol-integration", "cleave-process-tree"]
+++

# Multi-instance Omegon coordination — parallel work streams on the same repo

## Overview

> Parent: [Directive-Branch Lifecycle — git branch as the unified task boundary](directive-branch-lifecycle.md)
> Spawned from: "Should `implement` auto-checkout the created branch, or is that too disruptive for operators who prefer to stay on main?"

*To be explored.*

## Research

### Current multi-instance state — what breaks today

Running two Omegon instances against the same repo today produces several collision domains:

**1. Git working tree (critical)**
Two instances sharing a single checkout cannot both make file edits. `git checkout`, `git add`, `git commit` are inherently serial on a single working tree. This is the hardest constraint — it's not an Omegon problem, it's a git fundamental.

**2. SQLite memory database** (.pi/memory/facts.db)
WAL mode + `busy_timeout = 10000` provides basic concurrent read access, but two instances writing facts, reinforcements, and embeddings will produce contention. SQLite WAL allows concurrent readers with one writer — but two writers will block each other with 10s timeouts.

**3. Shared state** (extensions/lib/shared-state.ts)
`globalThis[Symbol.for("pi-kit-shared-state")]` is per-process. Two Omegon processes have completely independent shared state. This is actually fine — they don't coordinate in-memory. But it means dashboard state, cleave state, and effort state are per-instance.

**4. OpenSpec artifacts** (openspec/changes/*)
Two instances both running `/opsx:propose` or `/opsx:ff` on the same change directory will clobber each other's files. No locking.

**5. Design tree documents** (docs/*.md)
Two instances both calling `design_tree_update` on the same node will race on file writes. Last writer wins.

**6. Session state** (~/.pi/agent/sessions/)
Per-session files — not a collision risk since sessions are UUID-namespaced.

**7. Cleave subprocess management** (subprocess-tracker.ts)
The PID tracking file and orphan cleanup are per-process. Two instances running cleave would create independent subprocess trees. No cross-instance visibility.

### Git worktrees as the natural isolation boundary

Git worktrees solve the hardest problem — working tree contention — and they're already proven in cleave's child dispatch:

**Each instance gets its own worktree and branch.** The primary instance stays on `main` (or the operator's chosen branch). When a directive starts, a second instance can operate in a separate worktree checked out to the directive branch. Each worktree has its own:
- Working tree (independent file edits)
- HEAD ref (independent branch)
- Index (independent staging area)

**What git worktrees share:**
- Object database (`.git/objects/`) — efficient, no duplication
- Refs (branches, tags) — both instances see all branches
- Config (`.git/config`) — shared settings

**What worktrees DON'T solve:**
- SQLite memory DB — still shared via `.pi/memory/facts.db` in the repo root. A second worktree's `.pi/` is a symlink or separate copy depending on setup. This needs explicit handling.
- OpenSpec artifacts — they live in the repo tree, so each worktree has its own copy. This is actually GOOD — each directive's artifacts live in its own worktree's `openspec/changes/`.
- Design tree docs — also in the repo tree, so each worktree has independent copies. Changes merge when branches merge.

**The model:**
```
repo/                         ← main worktree (operator's primary instance)
  .git/
  .pi/memory/facts.db         ← shared SQLite (WAL handles concurrency)
  docs/
  extensions/
  openspec/

/tmp/omegon-worktrees/
  feature-foo/                 ← worktree for directive "foo" (second instance)
    docs/
    extensions/
    openspec/changes/foo/      ← directive's artifacts isolated here
```

This is exactly how cleave already works, but at the directive level instead of the child level.

### Three operational modes for multi-instance

**Mode A: Single instance, serial directives (current default)**
One Omegon instance. `implement` creates a branch and checks it out. Work happens serially. When the directive completes, archive merges to main. Simple, no coordination needed. This is what the parent node's directive-branch lifecycle describes.

**Mode B: Primary + delegate instances via worktrees**
The primary instance stays on main. When `implement` fires, it creates a worktree + branch and spawns a delegate Omegon instance in that worktree. The delegate does the spec work, cleave, assessment. When done, the primary merges the worktree branch back to main. This is cleave's model lifted to the directive level.

Advantages: operator keeps their primary instance for ad-hoc work. Directives run in isolation. Multiple directives can run in parallel.

Disadvantages: the delegate instance needs its own terminal/session. The operator needs to monitor multiple instances. This is the Omega coordinator vision.

**Mode C: Single instance, branch-aware checkout**
One instance, but it actively manages which branch it's on based on the active directive. `implement` checks out the directive branch. Session start detects the active directive and ensures the right branch. Archive merges and switches back to main. Between directives, the instance is on main.

This is the simplest version of the parent node's proposal. It answers the parent question directly: yes, `implement` should auto-checkout, because the branch IS the directive boundary.

**Recommendation:** Mode C first (it's the foundation), Mode B later (it's the Omega scaling story). Mode A is what we have now and it doesn't work well.

### SQLite WAL concurrency — what the guarantees actually are

**The one-sentence summary**: SQLite WAL mode allows unlimited concurrent readers with exactly one writer. A second writer blocks (up to `busy_timeout`) then gets `SQLITE_BUSY` — it does NOT corrupt data, it fails safely.

**What this means for Omegon:**

The memory database uses:
```
journal_mode = WAL
busy_timeout = 10000   (10 seconds)
```

With two Omegon instances sharing the same `facts.db`:
- **Both can READ simultaneously** — no contention, no blocking. This is the common case (context injection, memory_recall, memory_query).
- **One can WRITE while others READ** — WAL's primary advantage. Readers see a consistent snapshot while the writer appends to the WAL.
- **Two simultaneous WRITES** — the second writer retries for up to 10s. If the first write completes within that window (which it should — fact store operations are fast single-row INSERTs/UPDATEs), the second writer succeeds. If not, it gets `SQLITE_BUSY`.
- **No corruption** — SQLite's locking guarantees prevent data corruption regardless of contention. The worst case is a failed write that the application must handle.

**Current failure handling in factstore.ts:**

```typescript
function isSqliteError(error: unknown): boolean {
  if (!(error instanceof Error)) return false;
  return error.message.includes("SQLITE_") || 
         error.message.includes("database is locked");
}
```

Most write operations catch errors silently (reinforcement, decay updates). Fact creation would throw to the caller if SQLITE_BUSY exhausts the timeout — but `memory_store` in the extension catches and reports errors to the agent.

**Assessment for shared DB (Mode B):**
Two instances sharing `facts.db` is SAFE with current settings. SQLite WAL with `busy_timeout = 10000` handles the contention correctly. Writes are small and fast (single-row ops), so the 10s timeout is generous. The main risk is:

1. **Checkpoint contention** — WAL auto-checkpoints after 1000 pages of writes. During checkpoint, the writer blocks readers briefly. With two instances both writing, checkpoints happen more often but are still fast.
2. **Schema migrations** — if one instance upgrades the schema, the other may see unexpected columns/tables. This is a startup concern, not a runtime one.

**Assessment for isolated DB (each worktree gets its own):**
Eliminates all contention but introduces fact divergence. Instance A learns something in worktree A that instance B in worktree B doesn't know. On merge, facts.jsonl (the git-tracked transport) would need three-way merge. The current `merge=union` gitattribute handles this for JSONL.

**Recommendation**: Share the DB for Mode C (single instance, trivially safe). For Mode B (multi-instance), sharing is also safe given current write patterns, but consider an explicit SQLITE_BUSY retry wrapper around critical writes rather than relying solely on busy_timeout.

### Instance presence detection — mechanisms and tradeoffs

Three established approaches for detecting "is another instance already running on this resource":

**1. PID file** (`.pi/runtime/omegon.pid`)
Write process ID to a file at startup. Check if PID is alive before taking the lock.

| Pro | Con |
|-----|-----|
| Simple to implement | Stale on crash — PID file remains after unclean exit |
| Human-readable (`cat .pi/runtime/omegon.pid`) | PID recycling — OS may reuse the PID for a different process |
| Cross-platform (works on macOS, Linux, Windows) | Race condition — two instances can read "no PID" simultaneously |

Stale PID mitigation: read the PID, send signal 0 (`kill -0 $PID`) to check if process exists, verify it's actually Omegon (check `/proc/$PID/cmdline` on Linux, `ps` on macOS). This is fragile but widely used.

**2. Advisory file lock** (`flock()` / `fs.flock()`)
Acquire an exclusive lock on a file. The OS automatically releases the lock when the process exits (even on crash/SIGKILL).

| Pro | Con |
|-----|-----|
| Automatically released on crash — no stale locks | Not natively available in Node.js (no `fs.flock()`) |
| Atomic — no TOCTOU race | Requires native addon or shelling out to `flock` command |
| Battle-tested POSIX mechanism | Not supported on all filesystems (NFS is problematic) |
| Can be non-blocking (try-lock) | Windows uses different mechanism (`LockFileEx`) |

Node.js implementation options:
- `proper-lockfile` npm package — uses mkdir as atomic lock, with staleness detection
- `flock` CLI wrapper — `execFileSync("flock", ["--nonblock", lockfile, ...])` 
- Native addon — `better-sqlite3` already ships a native addon, so the dependency cost is already paid
- `fs.open()` with `O_EXCL` — atomic file creation, but no auto-release on crash

**3. Unix domain socket** (`.pi/runtime/omegon.sock`)
Bind a Unix socket at startup. Second instance connects to the socket to detect (and optionally communicate with) the first.

| Pro | Con |
|-----|-----|
| Auto-cleaned on process exit (mostly) | macOS/Linux only — no Windows |
| Enables IPC between instances | Socket file can remain after SIGKILL (stale) |
| Can pass structured messages | More complex than needed for presence detection |
| Foundation for Omega coordinator protocol | Requires cleanup on startup (unlink stale socket) |

**Recommendation for Omegon:**

**Use advisory file lock (proper-lockfile or equivalent) as the primary mechanism, with a PID file as a secondary human-readable signal.**

Rationale:
- Advisory lock is the only mechanism that handles crash cleanup automatically (the OS releases the lock when the process exits for any reason including SIGKILL)
- PID file supplements it for human debugging (`cat .pi/runtime/omegon.pid` to see who has the lock)
- Unix socket is overkill for presence detection alone, but becomes relevant when Omega needs IPC — defer it to that phase

**The workflow:**

1. On startup, try to acquire exclusive flock on `.pi/runtime/omegon.lock`
2. If acquired: write PID to `.pi/runtime/omegon.pid`, proceed normally
3. If NOT acquired: read PID file, detect the other instance
   - Offer: "Another Omegon instance (PID 12345) is active on this repo. Start in a separate worktree for directive X?" 
   - If yes: create worktree, launch there (Mode B)
   - If no: exit or run in read-only advisory mode

**File locations:**

`.pi/runtime/` is gitignored and per-repo. The lock is per-repo, not per-user — two users sharing a repo (via NFS or similar) would also be detected, which is the correct behavior since git working tree contention is the concern.

### Worktree memory isolation — gitignored files don't travel

**Tested empirically**: git worktrees only receive tracked files. Gitignored files (like `facts.db`, `facts.db-wal`, `facts.db-shm`) do NOT appear in the worktree.

This means a delegate Omegon instance in a worktree starts with NO memory database. The `FactStore` constructor creates a fresh one on first access (`fs.mkdirSync` + `new Database(path)`).

**Implications for the three approaches:**

**A. Shared DB via symlink**: The worktree startup could symlink `.pi/memory/facts.db` back to the primary worktree's copy. Both instances then share the same file — SQLite WAL handles the concurrency. Simple, but means the delegate can see/modify the primary's memory (probably desirable for project memory continuity).

**B. Copied DB**: Copy `facts.db` from the primary worktree to the delegate's `.pi/memory/` at worktree creation time. The delegate gets a snapshot of memory at fork time. Changes diverge. On merge, `facts.jsonl` (git-tracked) can three-way merge via `merge=union`. New facts discovered by the delegate would need to be imported back.

**C. Fresh DB with JSONL bootstrap**: The delegate starts fresh but imports `facts.jsonl` (which IS tracked and appears in the worktree). This gives the delegate the durable transport snapshot — all facts that were last exported — but loses any in-flight runtime state (reinforcement counts, embeddings, episodes not yet exported).

**For Mode C (single instance)**: Not relevant — same process, same DB.

**For Mode B (multi-instance)**: Option A (symlink) is simplest and gives the delegate full memory access. The SQLite WAL concurrency research shows this is safe for the read-heavy, write-light pattern of memory operations. Option C (JSONL bootstrap) is the cleanest isolation but loses runtime state.

**Recommendation**: Start with Option A (symlink) for Mode B. It's the simplest, gives full memory continuity, and SQLite WAL handles the concurrency safely. If contention proves problematic in practice (unlikely given write patterns), upgrade to Option C.

### Risk matrix — what can go wrong and how bad is it

**Risks ranked by severity × likelihood for multi-instance Mode B:**

| Risk | Severity | Likelihood | Mitigation |
|------|----------|------------|------------|
| Two instances editing same file in shared checkout | **Critical** — silent data corruption | **High** without detection | Instance presence lock prevents this entirely |
| SQLite SQLITE_BUSY on concurrent writes | **Low** — fails safely, no corruption | **Medium** with shared DB | busy_timeout=10s handles it; retry wrapper for defense-in-depth |
| Stale lock file after crash | **Medium** — blocks new instance | **Low** (advisory flock auto-releases on process death) | PID file + process liveness check as fallback |
| Schema migration race | **High** — table structure mismatch | **Very low** (only on Omegon version upgrades) | Startup sequence: acquire lock, migrate, release |
| WAL checkpoint under contention | **Low** — brief reader stall | **Low** | No mitigation needed; stalls are sub-second |
| PID recycling (stale PID matches new process) | **Low** — false positive detection | **Very low** | Check process name, not just PID existence |
| NFS/network filesystem flock failure | **Medium** — lock silently doesn't work | **Very low** (local dev is the primary target) | Document as unsupported; detect and warn |
| Worktree `.pi/` divergence | **Low** — config drift | **Medium** | Symlink `.pi/` or key subdirectories back to primary |

**The critical risk (shared checkout collision) is entirely eliminated by the instance presence lock.** Everything else is either low-severity, low-likelihood, or both. The SQLite concurrency concern is the most common question people have, but it's the most well-understood — WAL mode with busy_timeout is the standard answer and it works correctly.

**What this means for implementation priority:**
1. Instance presence detection (flock + PID) — eliminates the critical risk
2. Directive-branch checkout enforcement — eliminates the workflow drift
3. Worktree-based delegation — enables parallel directives (Mode B)
4. Memory DB handling — symlink for shared, with JSONL bootstrap as fallback

### Mind system vs symlink — using the built-in abstraction for worktree memory

The factstore already has a mind system designed for exactly this kind of scoped-memory relationship. Comparing the two approaches:

**Option A: Symlink `.pi/memory/facts.db` from worktree → primary**

Both instances share one SQLite file. Simple. But:
- Both instances see ALL facts from ALL minds, ALL episodes, ALL embeddings
- Writes from the delegate (new facts discovered during directive work) go into the shared DB immediately — there's no "directive-scoped" boundary
- If the delegate crashes mid-transaction, WAL recovery handles it, but there's no logical isolation between directive work and primary work
- No mechanism to "undo" what the delegate learned if the directive is abandoned (branch deleted without merge)

**Option B: Mind-per-directive using existing factstore API**

The mind system (`createMind`, `forkMind`, `ingestMind`, `setActiveMind`) was designed for exactly this:

1. When `implement` creates a directive, also: `store.forkMind('default', 'directive/node-id', 'Memory scope for feature/node-id')`
2. This copies all active facts from `default` into a new `directive/node-id` mind
3. The delegate instance sets its active mind: `store.setActiveMind('directive/node-id')`
4. All new facts the delegate discovers are scoped to this mind
5. On archive (directive complete): `store.ingestMind('directive/node-id', 'default')` merges discoveries back to the default mind, then retires the directive mind
6. On abandon (directive cancelled): `store.deleteMind('directive/node-id')` — clean removal, no pollution of default

**What exists today vs what's needed:**

| Capability | Status |
|---|---|
| `createMind(name, desc, opts)` | ✅ Implemented |
| `forkMind(source, target, desc)` | ✅ Implemented — copies all active facts |
| `ingestMind(source, target)` | ✅ Implemented — deduplicates by content_hash, retires source |
| `setActiveMind(name)` | ✅ Implemented — persisted in DB settings table |
| `getActiveMind()` | ✅ Implemented |
| `deleteMind(name)` | ✅ Implemented — cascades to all facts |
| Active mind used in context injection | ❌ NOT implemented — injection always reads from 'default' |
| Active mind used in memory_store | ❌ NOT implemented — stores always go to 'default' |
| Active mind used in memory_recall | ❌ NOT implemented — searches always search 'default' |
| `/memory link` for external DB | ❌ Stub — returns "being rebuilt for SQLite store" |

The infrastructure is built but not wired. The gap is in the memory extension's read/write paths — they all hardcode `'default'` as the mind name. Wiring `getActiveMind() ?? 'default'` into those paths would activate the entire system.

**The mind approach still needs a shared DB file for both instances.** The mind is a logical partition within one SQLite database, not a separate file. So the worktree delegate still needs to access the primary's `facts.db` — but instead of seeing everything in `default`, it works in its own `directive/node-id` mind namespace.

**This is strictly better than the symlink approach** because:
- Facts are logically scoped to the directive
- Abandon is clean (delete the mind, not "try to figure out which facts came from this directive")
- Merge is explicit (ingestMind deduplicates and retires)
- The isolation boundary matches the branch boundary
- It uses infrastructure that already exists and was designed for this purpose

**What needs to happen:**
1. Wire `getActiveMind()` into the memory extension's read/write paths (medium effort — grep for 'default' mind references)
2. Add `forkMind` + `setActiveMind` to the `implement` flow
3. Add `ingestMind` to the `archive` flow
4. Add `deleteMind` to the directive-abandon flow
5. The delegate worktree still symlinks `facts.db` for physical access, but operates in its own logical mind partition

### Context-gated memory and federation sync — avoid project overfit

The multi-instance model should not make every Omegon invocation behave like a long-running repository coordinator. Memory continuity has different costs depending on the current surface:

| Mode | Activation signal | Memory behavior | Project/federation behavior |
|---|---|---|---|
| One-off / non-Git | no `.git`, no project directives, ad hoc file/task | lightweight recall/store only when useful | no fetch/status, changelog, design docs, handoff, or sibling-checkout scan unless requested |
| Ordinary Git repo | `.git` exists, no Omegon lifecycle signals | use Git status/fetch only when relevant to the task | no Omegon-specific lifecycle artifacts unless project policy asks for them |
| Known lifecycle project | project directives, design tree, OpenSpec, changelog, or Omegon repo signals | store durable decisions and recall project facts at task boundaries | reconcile with docs/OpenSpec/changelog when behavior or decisions change |
| Multi-checkout / federation | explicit operator request or declared sibling-checkout topology | compare memory backend/mind state across checkouts | read-only status first; sync/merge only after explicit decision |

This keeps the memory system useful for small tasks without turning every one-off edit into a Git/lifecycle operation. Federation status must degrade to “not applicable” outside repositories instead of creating files or failing.

**First implementation pass:** add a read-only context projection that classifies the current directory into these modes and reports why. Do not perform synchronization, write handoff artifacts, or mutate memory backends in the first pass.

## Decisions

### Decision: Federation and memory-sync status starts as a read-only context projection

**Status:** decided
**Rationale:** The safe first implementation is detection, not synchronization. A read-only projection can answer “what context am I in, and what memory/project synchronization rules apply?” without risking overfit or unexpected writes. This projection becomes the shared input for future CLI/TUI/Workbench surfaces and can degrade cleanly for non-Git one-off tasks.

**Initial fields:**

- `cwd`
- `mode`: `one_off`, `ordinary_git`, `lifecycle_project`, or `federation`
- `signals`: detected reasons such as `.git`, `AGENTS.md`, `openspec/`, design docs, changelog, sibling checkouts
- `git`: optional branch/head/ahead-behind/dirty summary when applicable
- `memory`: backend identity if cheaply knowable; otherwise `unknown`
- `recommended_behavior`: short human-readable policy

**Non-goals for first pass:**

- automatic memory import/export
- sibling checkout mutation
- handoff document creation
- changelog/design/OpenSpec writes
- background daemon coordination

### Decision: implement should auto-checkout the directive branch (Mode C foundation)

**Status:** exploring
**Rationale:** The branch IS the directive boundary. If `implement` creates a branch but doesn't check it out, every subsequent operation (cleave, assess, archive) must independently figure out which branch to use, and the operator drifts to main by default. Auto-checkout makes the branch the natural working context. 

For multi-instance (Mode B), the directive branch lives in a worktree — checkout is per-worktree and doesn't affect the primary instance. So auto-checkout is correct for both modes.

The escape hatch for operators who want to stay on main: they simply don't use `implement`. Direct commits to main remain valid for lightweight bug/chore/task work. The lifecycle ceremony is opt-in at the `implement` gate.

### Decision: Parallel directives use git worktrees, not shared checkout

**Status:** exploring
**Rationale:** Git's working tree is a serial resource. Two directives modifying the same checkout will produce corrupt state. Worktrees are the proven isolation mechanism — cleave already demonstrates this. The scaling path from Mode C (single instance, serial) to Mode B (multi-instance, parallel) is worktrees, not shared checkout tricks. This aligns with the Omega coordinator vision where each directive is a supervised subprocess with its own worktree.

### Decision: Use mind-per-directive with shared DB rather than raw symlink

**Status:** exploring
**Rationale:** The factstore mind system (`forkMind`, `ingestMind`, `setActiveMind`, `deleteMind`) was designed for exactly this scoping pattern and is fully implemented at the storage layer. The gap is only in the memory extension's read/write paths which hardcode the 'default' mind. Wiring `getActiveMind()` into those paths activates logical isolation per-directive within a shared physical DB. This gives clean abandon (delete mind), clean merge (ingestMind with dedup), and directive-scoped fact discovery — none of which a raw symlink provides. The worktree delegate still needs physical access to the primary's `facts.db` (via symlink or path override), but operates in its own logical mind namespace rather than polluting the default mind.

### Decision: Mind-per-directive is implemented and wired end-to-end

**Status:** decided
**Rationale:** The factstore mind API was already fully implemented. The wiring required 110 lines across 4 files: shared-state type, implement fork+activate, archive ingest+delete, and memory queue drain. All existing memory_store/memory_recall/memory_query/context injection paths already used activeMind() — no changes needed there. The mind system provides logical isolation within a shared physical DB, clean abandon (deleteMind), and explicit merge with deduplication (ingestMind).

### Decision: Mode C (single-instance branch-aware) is the implementation target; Mode B and instance-presence detection are deferred to Omega

**Status:** decided
**Rationale:** Instance-presence detection (flock+PID) only matters when multiple Omegon instances target the same repo — Mode B. Mode C is single-instance. Building the presence-detection infrastructure now would be engineering for a scenario that doesn't exist yet and that naturally belongs to the Omega coordinator. The deliverables for this pass: (1) auto-checkout on implement, (2) branch↔mind consistency on session start, (3) dashboard indicator, (4) tests. Instance-presence and worktree delegation are Omega scope.

## Open Questions

*No open questions.*
