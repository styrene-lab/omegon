+++
id = "a5a92bdf-00b2-43c0-9a46-b95d47a71618"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Status Check

Quick session orientation. Load state from all subsystems and present a dashboard-style summary.

## 1. Load State (parallel)

Do all of these in parallel — they are independent:

- `memory_query` — get project memory (skim for recent sessions, open issues)
- `design_tree` action `list` — get design nodes
- `design_tree` action `frontier` — get open questions
- Check OpenSpec: `ls openspec/changes/ 2>/dev/null`
- Git state: `git branch --show-current && git status --short | head -10`

## 2. Parse and Summarize

From the loaded state, extract:

- **Design tree**: count by status (exploring/decided/seed), list any open questions
- **OpenSpec**: list active changes with task completion counts
- **Memory**: count facts, note the most recent session episode
- **Git**: current branch, dirty files count
- **Known issues**: any from memory's Known Issues section

## 3. Present

```
📊 Status — <project name>
━━━━━━━━━━━━━━━━━━━━━━━

🌳 Design: <N decided, N exploring, N open questions>
   <if open questions, list top 3 briefly>

📋 OpenSpec: <N active changes>
   <list each: name — M/N tasks complete>

🔀 Git: <branch> · <clean | N dirty files>
🧠 Memory: <N facts> · Last session: <date — topic>

⚠️  Open Issues:
   <top 2-3 known issues from memory, one line each>
   <or "None tracked">
```

If there are actionable next steps (incomplete OpenSpec changes, open design questions, dirty working tree), suggest 1-2 concrete actions at the bottom.

Keep the entire output under 20 lines. This is a glanceable dashboard, not a report.
