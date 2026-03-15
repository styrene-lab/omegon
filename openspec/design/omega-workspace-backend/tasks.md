# Omega WorkspaceBackend — local worktree vs. k8s pod storage — Design Tasks

## 1. Open Questions

- [ ] 1.1 For k8s pod workspaces, which storage backend is the v1 default: shared PVC (simpler, local k8s friendly) or branch clone (works on any cluster, requires a git remote)? The answer determines whether Omega needs a git remote configured in its k8s backend, or just a PVC claim name.
