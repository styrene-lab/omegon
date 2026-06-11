#!/usr/bin/env python3
"""Unit tests for dirty_report.py."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from scripts import dirty_report as dr  # noqa: E402


class DirtyReportTests(unittest.TestCase):
    def test_audit_log_is_agent_state_not_source(self) -> None:
        category, plane, _ = dr.classify(".omegon/audit-log.jsonl")
        self.assertEqual(category, "agent-state")
        self.assertEqual(plane, "agent-state")

    def test_runtime_sqlite_files_are_agent_state(self) -> None:
        for path in [
            ".omegon/codescan.db",
            ".omegon/codescan.db-wal",
            ".omegon/codescan.db-shm",
            ".omegon/ipc.sock",
            ".omegon/runtime/session.json",
        ]:
            with self.subTest(path=path):
                category, plane, _ = dr.classify(path)
                self.assertEqual(category, "agent-state")
                self.assertEqual(plane, "agent-state")

    def test_unclassified_omegon_jsonl_is_source_until_decided(self) -> None:
        category, plane, _ = dr.classify(".omegon/evidence/records.jsonl")
        self.assertEqual(category, "omegon-local")
        self.assertEqual(plane, "source")

    def test_arbitrary_jsonl_is_not_agent_state(self) -> None:
        category, plane, _ = dr.classify("notes/foo.jsonl")
        self.assertEqual(category, "other")
        self.assertEqual(plane, "source")

    def test_source_clean_ignores_only_agent_state_entries(self) -> None:
        entries = [
            dr.Entry(
                status=" M",
                path=".omegon/audit-log.jsonl",
                category="agent-state",
                plane="agent-state",
                note="live",
            )
        ]
        report = dr.build_report(entries)
        self.assertTrue(report["source_clean"])
        self.assertTrue(report["agent_state_dirty"])

    def test_source_clean_fails_for_source_entries(self) -> None:
        entries = [
            dr.Entry(
                status=" M",
                path="core/crates/omegon/src/lib.rs",
                category="rust-source",
                plane="source",
                note="source",
            ),
            dr.Entry(
                status=" M",
                path=".omegon/audit-log.jsonl",
                category="agent-state",
                plane="agent-state",
                note="live",
            ),
        ]
        report = dr.build_report(entries)
        self.assertFalse(report["source_clean"])
        self.assertTrue(report["agent_state_dirty"])

    def test_source_clean_fails_for_staged_agent_state(self) -> None:
        entries = [
            dr.Entry(
                status="M ",
                path=".omegon/audit-log.jsonl",
                category="agent-state",
                plane="agent-state",
                note="live",
            )
        ]
        report = dr.build_report(entries)
        self.assertTrue(report["source_clean"])
        self.assertEqual(report["staged_agent_state_count"], 1)


if __name__ == "__main__":
    unittest.main()
