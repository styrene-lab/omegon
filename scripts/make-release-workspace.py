#!/usr/bin/env python3
"""Write a release workspace.json for just cut-rc.

Usage: make-release-workspace.py <wsdir> <milestone> <created_at>
"""
import json
import sys
from pathlib import Path

wsdir, milestone, created_at = sys.argv[1], sys.argv[2], sys.argv[3]

data = {
    "project_id": "Users::cwilson::workspace::black-meridian::omegon",
    "workspace_id": "cut-rc-workspace",
    "label": "release",
    "path": wsdir,
    "backend_kind": "local-dir",
    "vcs_ref": {"vcs": "git", "branch": "main", "revision": None, "remote": "origin"},
    "bindings": {"milestone_id": milestone, "design_node_id": None, "openspec_change": None},
    "branch": "main",
    "role": "release",
    "workspace_kind": "release",
    "mutability": "mutable",
    "owner_session_id": "cut-rc",
    "owner_agent_id": "operator",
    "created_at": created_at,
    "last_heartbeat": created_at,
    "archived": False,
    "archived_at": None,
    "archive_reason": None,
    "parent_workspace_id": None,
    "source": "operator",
}

out = Path(wsdir) / ".omegon" / "runtime" / "workspace.json"
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text(json.dumps(data, indent=2) + "\n")
print(f"  wrote {out}")
