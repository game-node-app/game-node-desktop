from __future__ import annotations

from pathlib import Path
import tempfile
import unittest

from scripts import spec_progress


SAMPLE_TASKS_MD = """# Tasks

Overview of the change.

## 1. Setup

- [x] 1.1 First task done
  - Completion (2026-11-14): Finished first task.
- [x] 1.2 Second task done
- [ ] 1.3 Third task pending

## 2. Advanced

- [ ] 2.1 Fourth task pending
- [ ] 2.2 Fifth task pending
"""

SAMPLE_DESIGN_MD = """# Design

## Context
Background details here.

## Decisions

### 1. Registration
Details of decision 1.

### 2. Protocol
Details of decision 2 across multiple lines.
More protocol specs.

### 11. Launch Overlays
Details of decision 11.
"""


class SpecProgressTests(unittest.TestCase):
    def test_normalize_tasks_and_grouping(self):
        raw = [
            {"id": "1", "description": "1.1 First task", "done": True, "line": 5},
            {"id": "2", "description": "1.2 Second task", "done": False, "line": 8},
            {"id": "3", "description": "7.7 Seventh task", "done": False, "line": 15},
            {"id": "4", "description": "Unnumbered task", "done": False, "line": 20},
        ]
        norm = spec_progress.normalize_tasks(raw)
        self.assertEqual(norm[0]["task_number"], "1.1")
        self.assertEqual(norm[0]["section"], "1")
        self.assertEqual(norm[2]["task_number"], "7.7")
        self.assertEqual(norm[2]["section"], "7")
        self.assertEqual(norm[3]["task_number"], "4")
        self.assertEqual(norm[3]["section"], "4")

        grouped = spec_progress.group_tasks_by_section(norm)
        self.assertEqual(list(grouped.keys()), ["1", "7", "4"])
        self.assertEqual(len(grouped["1"]), 2)

    def test_fallback_tasks_parsing(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            f = Path(tmpdir) / "tasks.md"
            f.write_text(SAMPLE_TASKS_MD, encoding="utf-8")

            payload = spec_progress.parse_tasks_file_fallback(f, "test-change")
            self.assertEqual(payload["changeName"], "test-change")
            self.assertEqual(payload["progress"]["total"], 5)
            self.assertEqual(payload["progress"]["complete"], 2)
            self.assertEqual(payload["progress"]["remaining"], 3)

            tasks = payload["tasks"]
            self.assertEqual(len(tasks), 5)
            self.assertTrue(tasks[0]["done"])
            self.assertEqual(tasks[0]["line"], 7)
            self.assertFalse(tasks[2]["done"])
            self.assertEqual(tasks[2]["line"], 10)

    def test_format_status(self):
        payload = {
            "changeName": "sample-change",
            "progress": {"total": 5, "complete": 2, "remaining": 3},
            "tasks": [
                {"id": "1", "description": "1.1 Done task", "done": True, "line": 7},
                {"id": "2", "description": "1.2 Done task", "done": True, "line": 9},
                {"id": "3", "description": "1.3 Pending task", "done": False, "line": 10},
                {"id": "4", "description": "2.1 Pending task", "done": False, "line": 14},
                {"id": "5", "description": "2.2 Pending task", "done": False, "line": 15},
            ],
        }

        # Default: pending only
        out = spec_progress.format_status(payload)
        self.assertIn("sample-change", out)
        self.assertIn("2/5 complete (40.0%), 3 remaining", out)
        self.assertIn("Next pending: 1.3 (line 10)", out)
        self.assertIn("[Section 1] 1 pending", out)
        self.assertIn("[Section 2] 2 pending", out)

        # Section filter
        out_sec2 = spec_progress.format_status(payload, section_filter="2")
        self.assertNotIn("[Section 1]", out_sec2)
        self.assertIn("[Section 2]", out_sec2)

        # Show all
        out_all = spec_progress.format_status(payload, show_all=True)
        self.assertIn("2 done, 1 pending", out_all)

    def test_get_task_slice(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            f = Path(tmpdir) / "tasks.md"
            f.write_text(SAMPLE_TASKS_MD, encoding="utf-8")
            payload = spec_progress.parse_tasks_file_fallback(f, "test-change")

            # By task number 1.1
            start, end, content = spec_progress.get_task_slice("1.1", payload, tasks_file=f)
            self.assertEqual(start, 7)
            self.assertEqual(end, 8)
            self.assertIn("- [x] 1.1 First task done", content)
            self.assertIn("Completion (2026-11-14)", content)

            # By sequential id 3 (which is 1.3)
            start3, end3, content3 = spec_progress.get_task_slice("3", payload, tasks_file=f)
            self.assertEqual(start3, 10)
            self.assertIn("- [ ] 1.3 Third task pending", content3)

            # Unknown task
            with self.assertRaises(KeyError):
                spec_progress.get_task_slice("9.9", payload, tasks_file=f)

    def test_markdown_outline_and_section_content(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            f = Path(tmpdir) / "design.md"
            f.write_text(SAMPLE_DESIGN_MD, encoding="utf-8")

            headings = spec_progress.parse_markdown_headings(f)
            self.assertEqual(len(headings), 6)
            self.assertEqual(headings[0].title, "Design")
            self.assertEqual(headings[0].level, 1)

            # Decision 1
            h_dec1 = headings[3]
            self.assertEqual(h_dec1.section_number, "1")
            self.assertEqual(h_dec1.title, "1. Registration")

            # Decision 11
            h_dec11 = headings[5]
            self.assertEqual(h_dec11.section_number, "11")
            self.assertEqual(h_dec11.title, "11. Launch Overlays")

            # Format outline
            formatted = spec_progress.format_outline(headings, f)
            self.assertIn("### 1. Registration", formatted)
            self.assertIn("### 11. Launch Overlays", formatted)

            # Extract section 11
            start, end, content = spec_progress.get_section_content(headings, "11", f)
            self.assertIn("### 11. Launch Overlays", content)
            self.assertIn("Details of decision 11.", content)
            self.assertNotIn("1. Registration", content)

            # Extract by partial title match
            start_p, end_p, content_p = spec_progress.get_section_content(headings, "protocol", f)
            self.assertIn("### 2. Protocol", content_p)


if __name__ == "__main__":
    unittest.main()
