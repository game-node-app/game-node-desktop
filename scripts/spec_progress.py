#!/usr/bin/env python3
"""Spec navigation and task progress helper for OpenSpec changes.

Reduces model context consumption and repetitive file re-reading:
- Queries OpenSpec structured apply progress without manual grep/file reads.
- Displays pending tasks grouped by section, next task, and exact line numbers.
- Extracts single task blocks from tasks.md on demand.
- Outlines large markdown artifacts (e.g. design.md) with line ranges and sizes,
  allowing targeted retrieval of individual sections instead of dumping 150KB+.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
from typing import Any, Dict, List, Optional, Tuple


REPO_ROOT = Path(__file__).resolve().parents[1]


def detect_default_change(repo_root: Path = REPO_ROOT) -> str:
    """Resolve one active change, requiring an explicit choice when ambiguous."""
    try:
        proc = subprocess.run(
            ["openspec", "list", "--json"],
            cwd=repo_root,
            capture_output=True,
            text=True,
            check=True,
        )
    except FileNotFoundError:
        proc = None

    if proc is not None:
        data = json.loads(proc.stdout)
        changes = data.get("changes", [])
        in_progress = [c for c in changes if c.get("status") == "in-progress"]
        if len(in_progress) == 1:
            return in_progress[0]["name"]
        if len(in_progress) > 1:
            names = ", ".join(sorted(c["name"] for c in in_progress))
            raise ValueError(f"Multiple OpenSpec changes are in progress ({names}); pass --change.")
        if len(changes) == 1:
            return changes[0]["name"]
        if changes:
            names = ", ".join(sorted(c["name"] for c in changes))
            raise ValueError(f"No unique in-progress OpenSpec change ({names}); pass --change.")
        raise FileNotFoundError("OpenSpec reports no changes; pass --change.")

    # Filesystem fallback is used only when the OpenSpec executable is absent.
    changes_dir = repo_root / "openspec" / "changes"
    if changes_dir.is_dir():
        candidates = [
            d.name
            for d in changes_dir.iterdir()
            if d.is_dir() and (d / "tasks.md").is_file() and d.name != "archive"
        ]
        if len(candidates) == 1:
            return candidates[0]
        if candidates:
            names = ", ".join(sorted(candidates))
            raise ValueError(f"Multiple OpenSpec changes are available ({names}); pass --change.")

    raise FileNotFoundError("No OpenSpec change with tasks.md was found; pass --change.")


def fetch_apply_instructions(change: str, repo_root: Path = REPO_ROOT) -> Dict[str, Any]:
    """Fetch structured instructions apply payload via openspec CLI or fallback parser."""
    try:
        proc = subprocess.run(
            ["openspec", "instructions", "apply", "--change", change, "--json"],
            cwd=repo_root,
            capture_output=True,
            text=True,
            check=True,
        )
    except FileNotFoundError:
        # Fall back only when OpenSpec is not installed; real CLI/JSON errors must stay visible.
        tasks_file = repo_root / "openspec" / "changes" / change / "tasks.md"
        if not tasks_file.is_file():
            raise FileNotFoundError(f"tasks.md not found for change {change!r} at {tasks_file}")
        return parse_tasks_file_fallback(tasks_file, change)
    return json.loads(proc.stdout)


def parse_tasks_file_fallback(tasks_file: Path, change: str) -> Dict[str, Any]:
    """Parse tasks.md directly when openspec CLI is not reachable."""
    lines = tasks_file.read_text(encoding="utf-8").splitlines()
    tasks: List[Dict[str, Any]] = []
    task_idx = 1
    task_re = re.compile(r"^\s*-\s*\[([ xX])\]\s*(.*)$")

    for idx, line in enumerate(lines, start=1):
        m = task_re.match(line)
        if m:
            is_done = m.group(1).lower() == "x"
            desc = m.group(2).strip()
            tasks.append({
                "id": str(task_idx),
                "description": desc,
                "done": is_done,
                "sourcePath": str(tasks_file),
                "line": idx,
            })
            task_idx += 1

    total = len(tasks)
    complete = sum(1 for t in tasks if t["done"])
    return {
        "changeName": change,
        "changeDir": str(tasks_file.parent),
        "progress": {
            "total": total,
            "complete": complete,
            "remaining": total - complete,
        },
        "tasks": tasks,
    }


def normalize_tasks(raw_tasks: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    """Enrich task dictionaries with task_number (e.g. '7.7') and section ('7')."""
    normalized: List[Dict[str, Any]] = []
    num_re = re.compile(r"^(\d+(?:\.\d+)*)\b")

    for t in raw_tasks:
        desc = t.get("description", "")
        m = num_re.match(desc)
        task_num = m.group(1) if m else str(t.get("id", ""))
        sec = task_num.split(".")[0] if "." in task_num else task_num
        normalized.append({
            **t,
            "task_number": task_num,
            "section": sec,
        })
    return normalized


def group_tasks_by_section(tasks: List[Dict[str, Any]]) -> Dict[str, List[Dict[str, Any]]]:
    """Group normalized tasks by their section prefix, preserving appearance order."""
    groups: Dict[str, List[Dict[str, Any]]] = {}
    for t in tasks:
        sec = t.get("section", "?")
        groups.setdefault(sec, []).append(t)
    return groups


def format_status(
    payload: Dict[str, Any],
    section_filter: Optional[str] = None,
    show_all: bool = False,
) -> str:
    """Format human-readable progress status."""
    change = payload.get("changeName", "unknown")
    progress = payload.get("progress", {})
    total = progress.get("total", 0)
    complete = progress.get("complete", 0)
    remaining = progress.get("remaining", total - complete)
    pct = (complete / total * 100.0) if total > 0 else 0.0

    raw_tasks = payload.get("tasks", [])
    tasks = normalize_tasks(raw_tasks)

    if section_filter:
        tasks = [t for t in tasks if t.get("section") == section_filter]

    pending = [t for t in tasks if not t.get("done")]

    out: List[str] = []
    out.append(f"OpenSpec change: {change}")
    if section_filter:
        section_total = len(tasks)
        section_complete = sum(1 for task in tasks if task["done"])
        out.append(
            f"Section {section_filter}: {section_complete}/{section_total} complete, "
            f"{section_total - section_complete} pending"
        )
        out.append(
            f"Change-wide progress: {complete}/{total} complete ({pct:.1f}%), {remaining} remaining"
        )
    else:
        out.append(f"Progress: {complete}/{total} complete ({pct:.1f}%), {remaining} remaining")

    if pending and not section_filter:
        next_task = pending[0]
        desc_preview = next_task['description'][:90] + ("..." if len(next_task['description']) > 90 else "")
        out.append(f"Next pending: {next_task['task_number']} (line {next_task['line']}): {desc_preview}")

    out.append("")

    active_tasks = tasks if show_all else pending
    grouped = group_tasks_by_section(active_tasks)

    if not grouped:
        out.append("No tasks matching filter.")
        return "\n".join(out)

    for sec, sec_tasks in grouped.items():
        sec_pending = [t for t in sec_tasks if not t.get("done")]
        sec_done = [t for t in sec_tasks if t.get("done")]
        status_tag = f"{len(sec_pending)} pending"
        if show_all:
            status_tag = f"{len(sec_done)} done, {len(sec_pending)} pending"

        line_min = min(t["line"] for t in sec_tasks)
        line_max = max(t["line"] for t in sec_tasks)
        items_str = ", ".join(t["task_number"] for t in sec_tasks)
        out.append(f"[Section {sec}] {status_tag} (lines {line_min}-{line_max}): {items_str}")

    return "\n".join(out)


def get_task_slice(
    task_id: str,
    payload: Dict[str, Any],
    tasks_file: Optional[Path] = None,
) -> Tuple[int, int, str]:
    """Retrieve the exact line range and markdown content for a given task."""
    raw_tasks = payload.get("tasks", [])
    tasks = normalize_tasks(raw_tasks)

    # Match by task_number (e.g. '7.7') or id (e.g. '46')
    target_idx: Optional[int] = None
    for i, t in enumerate(tasks):
        if t.get("task_number") == task_id or str(t.get("id")) == task_id:
            target_idx = i
            break

    if target_idx is None:
        raise KeyError(f"Task {task_id!r} not found in change {payload.get('changeName')}")

    target = tasks[target_idx]
    start_line = target["line"]

    if tasks_file is None:
        src = target.get("sourcePath")
        if src:
            tasks_file = Path(src)
        else:
            tasks_file = REPO_ROOT / "openspec" / "changes" / payload["changeName"] / "tasks.md"

    lines = tasks_file.read_text(encoding="utf-8").splitlines()

    # The task ends before the next task's start line, or next section heading, or EOF
    if target_idx + 1 < len(tasks):
        end_line = tasks[target_idx + 1]["line"] - 1
    else:
        end_line = len(lines)

    # Trim trailing blank lines in range
    while end_line > start_line and not lines[end_line - 1].strip():
        end_line -= 1

    chunk = "\n".join(lines[start_line - 1 : end_line])
    return start_line, end_line, chunk


class MarkdownHeading:
    def __init__(self, level: int, title: str, start_line: int):
        self.level = level
        self.title = title
        self.start_line = start_line
        self.end_line = start_line
        self.byte_size = 0
        self.line_count = 0
        self.section_number: Optional[str] = None
        # Extract section number if present e.g. "### 11. Standard launch" -> "11"
        m = re.match(r"^(\d+(?:\.\d+)*)\b", title)
        if m:
            self.section_number = m.group(1)

    def to_dict(self) -> Dict[str, Any]:
        return {
            "level": self.level,
            "title": self.title,
            "section_number": self.section_number,
            "start_line": self.start_line,
            "end_line": self.end_line,
            "byte_size": self.byte_size,
            "line_count": self.line_count,
        }


def parse_markdown_headings(file_path: Path) -> List[MarkdownHeading]:
    """Parse markdown headings with line ranges and byte sizes."""
    text = file_path.read_text(encoding="utf-8")
    lines = text.splitlines(keepends=True)
    headings: List[MarkdownHeading] = []
    heading_re = re.compile(r"^(#{1,6})\s+(.*)$")
    fence_open_re = re.compile(r"^ {0,3}(`{3,}|~{3,})(.*)$")
    fence_close_re = re.compile(r"^ {0,3}(`+|~+)\s*$")
    fence_char: Optional[str] = None
    fence_size = 0

    for idx, raw_line in enumerate(lines, start=1):
        stripped = raw_line.rstrip("\r\n")
        if fence_char is not None:
            closing = fence_close_re.match(stripped)
            if (
                closing
                and closing.group(1)[0] == fence_char
                and len(closing.group(1)) >= max(fence_size, 3)
            ):
                fence_char = None
                fence_size = 0
            continue

        opening = fence_open_re.match(stripped)
        if opening:
            fence_char = opening.group(1)[0]
            fence_size = len(opening.group(1))
            continue

        m = heading_re.match(stripped)
        if m:
            level = len(m.group(1))
            title = m.group(2).strip()
            headings.append(MarkdownHeading(level, title, idx))

    total_lines = len(lines)
    for i, h in enumerate(headings):
        if i + 1 < len(headings):
            h.end_line = headings[i + 1].start_line - 1
        else:
            h.end_line = total_lines

        chunk = "".join(lines[h.start_line - 1 : h.end_line])
        h.byte_size = len(chunk.encode("utf-8"))
        h.line_count = (h.end_line - h.start_line) + 1

    return headings


def format_outline(headings: List[MarkdownHeading], file_path: Path) -> str:
    """Format markdown outline with line ranges and human-readable byte sizes."""
    total_bytes = file_path.stat().st_size

    out: List[str] = []
    out.append(f"Outline for {file_path.relative_to(REPO_ROOT) if file_path.is_relative_to(REPO_ROOT) else file_path} "
               f"({total_bytes / 1024:.1f} KB, {headings[-1].end_line if headings else 0} lines):")
    out.append("")

    for h in headings:
        indent = "  " * (h.level - 1)
        kb = h.byte_size / 1024.0
        size_str = f"{kb:.1f} KB" if kb >= 1.0 else f"{h.byte_size} B"
        out.append(f"{indent}{'#' * h.level} {h.title}  [L{h.start_line}-L{h.end_line}, {h.line_count} lines, {size_str}]")

    return "\n".join(out)


def get_section_content(
    headings: List[MarkdownHeading],
    section_query: str,
    file_path: Path,
) -> Tuple[int, int, str]:
    """Retrieve specific section content by section number or title substring."""
    target: Optional[MarkdownHeading] = None
    query_clean = section_query.strip().lower()

    # Exact section number match first
    for h in headings:
        if h.section_number == section_query:
            target = h
            break

    # Substring in title
    if target is None:
        for h in headings:
            if query_clean in h.title.lower():
                target = h
                break

    if target is None:
        available = ", ".join(h.section_number for h in headings if h.section_number)
        raise KeyError(f"Section {section_query!r} not found. Available numbered sections: {available}")

    text = file_path.read_text(encoding="utf-8")
    lines = text.splitlines()
    chunk = "\n".join(lines[target.start_line - 1 : target.end_line])
    return target.start_line, target.end_line, chunk


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="OpenSpec progress and spec navigation helper (reduces model context consumption).",
    )
    sub = parser.add_subparsers(dest="command")

    # status subcommand
    status_p = sub.add_parser("status", help="Show change progress and pending tasks by section.")
    status_p.add_argument("--change", help="OpenSpec change name (defaults to auto-detected active change).")
    status_p.add_argument("--section", "-s", help="Filter pending tasks to a specific section number.")
    status_p.add_argument("--all", action="store_true", help="Include completed tasks in the output.")
    status_p.add_argument("--json", action="store_true", help="Output raw JSON.")

    # next subcommand
    next_p = sub.add_parser(
        "next",
        help="Show progress and the full next pending task in one helper invocation.",
    )
    next_p.add_argument("--change", help="OpenSpec change name (defaults to the unique active change).")
    next_p.add_argument("--section", "-s", help="Choose the next pending task in this task section.")
    next_p.add_argument(
        "--design-section",
        help="Also print a numbered section or title from design.md (for example, 11).",
    )

    # show subcommand
    show_p = sub.add_parser("show", help="Show the exact task block from tasks.md without loading the entire file.")
    show_p.add_argument("task_id", help="Task ID (e.g. '7.7' or global sequential id like '46').")
    show_p.add_argument("--change", help="OpenSpec change name.")
    show_p.add_argument("--line-numbers", "-n", action="store_true", help="Prefix output lines with line numbers.")

    # outline subcommand
    outline_p = sub.add_parser("outline", help="Outline markdown headings with line ranges/sizes or extract one section.")
    outline_p.add_argument("file", nargs="?", help="Path to markdown file (defaults to design.md of active change).")
    outline_p.add_argument("--change", help="OpenSpec change name (used to resolve default design.md).")
    outline_p.add_argument("--section", "-s", help="Extract and print only this section number or title.")
    outline_p.add_argument("--json", action="store_true", help="Output heading tree as JSON.")

    return parser


def main(argv: Optional[List[str]] = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    cmd = args.command or "status"

    if cmd == "status":
        try:
            change = args.change or detect_default_change()
        except (ValueError, FileNotFoundError) as err:
            parser.error(str(err))
        data = fetch_apply_instructions(change)
        if args.json:
            print(json.dumps(data, indent=2))
        else:
            print(format_status(data, section_filter=args.section, show_all=args.all))
        return 0

    if cmd == "next":
        try:
            change = args.change or detect_default_change()
        except (ValueError, FileNotFoundError) as err:
            parser.error(str(err))
        data = fetch_apply_instructions(change)
        print(format_status(data, section_filter=args.section))

        pending = [task for task in normalize_tasks(data.get("tasks", [])) if not task.get("done")]
        if args.section:
            pending = [task for task in pending if task.get("section") == args.section]
        if pending:
            task = pending[0]
            start_line, end_line, content = get_task_slice(task["task_number"], data)
            print(f"\n--- Next task {task['task_number']} (lines {start_line}-{end_line}) ---")
            print(content)
        else:
            print("\nNo pending task matches the selected section.")

        if args.design_section:
            design_file = REPO_ROOT / "openspec" / "changes" / change / "design.md"
            if not design_file.is_file():
                sys.stderr.write(f"Error: design.md not found for change {change!r} at {design_file}\n")
                return 1
            headings = parse_markdown_headings(design_file)
            try:
                start_line, end_line, content = get_section_content(
                    headings, args.design_section, design_file
                )
            except KeyError as err:
                sys.stderr.write(f"Error: {err}\n")
                return 1
            print(
                f"\n--- Design section {args.design_section} "
                f"(lines {start_line}-{end_line}) ---"
            )
            print(content)
        return 0

    if cmd == "show":
        try:
            change = args.change or detect_default_change()
        except (ValueError, FileNotFoundError) as err:
            parser.error(str(err))
        data = fetch_apply_instructions(change)
        try:
            start_line, end_line, content = get_task_slice(args.task_id, data)
        except KeyError as err:
            sys.stderr.write(f"Error: {err}\n")
            return 1

        print(f"--- Task {args.task_id} (lines {start_line}-{end_line}) ---")
        if args.line_numbers:
            for i, line in enumerate(content.splitlines(), start=start_line):
                print(f"{i:4d}: {line}")
        else:
            print(content)
        return 0

    if cmd == "outline":
        try:
            change = args.change or detect_default_change()
        except (ValueError, FileNotFoundError) as err:
            parser.error(str(err))
        if args.file:
            target_path = Path(args.file)
            if not target_path.is_absolute():
                target_path = REPO_ROOT / target_path
        else:
            target_path = REPO_ROOT / "openspec" / "changes" / change / "design.md"

        if not target_path.is_file():
            sys.stderr.write(f"Error: File not found: {target_path}\n")
            return 1

        headings = parse_markdown_headings(target_path)

        if args.section:
            try:
                start_l, end_l, chunk = get_section_content(headings, args.section, target_path)
            except KeyError as err:
                sys.stderr.write(f"Error: {err}\n")
                return 1
            print(f"--- Section {args.section} from {target_path.name} (lines {start_l}-{end_l}) ---")
            print(chunk)
            return 0

        if args.json:
            print(json.dumps([h.to_dict() for h in headings], indent=2))
        else:
            print(format_outline(headings, target_path))
        return 0

    parser.print_help()
    return 0


if __name__ == "__main__":
    sys.exit(main())
