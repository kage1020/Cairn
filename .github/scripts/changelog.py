#!/usr/bin/env python3
"""Check and assemble the per-PR changelog fragments in `changelog.d/`.

A pull request that has something to tell a reader of CHANGELOG.md adds one
file, `changelog.d/<slug>.<kind>.md`, instead of editing CHANGELOG.md. Two PRs
never touch the same file, so they never conflict over the changelog. The
release workflow folds every fragment into the section release-plz wrote for
the new version, and deletes them, in the release PR itself.

    changelog.py check                 every fragment is well formed
    changelog.py pr --title T --base B the PR's title and fragments agree
    changelog.py assemble              fold the fragments into CHANGELOG.md

Standard library only: it runs on a bare `ubuntu-latest` runner.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FRAGMENT_DIR = ROOT / "changelog.d"
CHANGELOG = ROOT / "CHANGELOG.md"

# Kind to heading, in the order the headings appear in a release section.
# `Added` to `Build` are the groups `commit_parsers` in release-plz.toml
# writes, spelled as its `group | upper_first` renders them, so a fragment
# lands under the heading release-plz already opened for the same PR's title.
# `Breaking changes` and `Deprecations` are the two sections
# `spec/compatibility` "How a break is communicated" requires; no commit type
# produces them, so only a fragment can.
KINDS = {
    "breaking": "Breaking changes",
    "deprecations": "Deprecations",
    "added": "Added",
    "changed": "Changed",
    "fixed": "Fixed",
    "performance": "Performance",
    "build": "Build",
}

FRAGMENT_NAME = re.compile(r"^[a-z0-9][a-z0-9-]*\.(?P<kind>[a-z]+)\.md$")
BREAKING_TITLE = re.compile(r"^[a-z]+(\([^)]*\))?!:")
# A PR link is the one place a number may appear: it is what release-plz
# writes after a title line, and it reads as a link, not as a coordinate.
PR_LINK = re.compile(r"\[#(\d+)\]\(https://github\.com/kage1020/Cairn/pull/\1\)")
BARE_NUMBER = re.compile(r"(?<![\w&])#\d+\b")
CI_ONLY_SCOPE = re.compile(r"^- \*\(ci\)\*")


def error(path: Path, line: int | None, message: str) -> None:
    shown = path.relative_to(ROOT) if path.is_absolute() else path
    if os.environ.get("GITHUB_ACTIONS") == "true":
        where = f"file={shown}" + (f",line={line}" if line else "")
        print(f"::error {where}::{message}")
    else:
        print(f"{shown}{f':{line}' if line else ''}: {message}", file=sys.stderr)


def fragments() -> list[Path]:
    if not FRAGMENT_DIR.is_dir():
        return []
    return sorted(p for p in FRAGMENT_DIR.iterdir() if p.name != "README.md")


def check_fragment(path: Path) -> bool:
    """Report what is wrong with one fragment; true when nothing is."""
    match = FRAGMENT_NAME.match(path.name)
    if not path.is_file() or not match:
        error(path, None, "a fragment is named `<slug>.<kind>.md`, slug in lowercase "
              "letters, digits and `-`, kind one of " + ", ".join(KINDS))
        return False
    if match["kind"] not in KINDS:
        error(path, None, f"unknown kind `{match['kind']}`; one of " + ", ".join(KINDS))
        return False

    ok = True
    lines = path.read_text(encoding="utf-8").splitlines()
    if not any(line.strip() for line in lines):
        error(path, None, "the fragment is empty")
        return False
    first = next(i for i, line in enumerate(lines) if line.strip())
    if not lines[first].startswith("- "):
        error(path, first + 1, "a fragment starts with a `- ` list item")
        ok = False
    for number, line in enumerate(lines, start=1):
        if line.strip() and not (line.startswith("- ") or line.startswith("  ")):
            error(path, number, "every line is a `- ` item or indented under one; "
                  "the heading comes from the file name")
            ok = False
        if BARE_NUMBER.search(PR_LINK.sub("", line)):
            error(path, number, "no issue or PR number in the tree: say what changed, "
                  "so a reader need not open GitHub to learn it")
            ok = False
        if CI_ONLY_SCOPE.match(line):
            error(path, number, "a `ci` change has no entry of its own; release-plz "
                  "skips the `ci` group too")
            ok = False
    return ok


def check(_: argparse.Namespace) -> int:
    results = [check_fragment(path) for path in fragments()]
    return 0 if all(results) else 1


def git(*args: str) -> list[str]:
    out = subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True, text=True)
    return [line for line in out.stdout.splitlines() if line]


def pr(args: argparse.Namespace) -> int:
    changed = git("diff", "--name-status", "--no-renames", f"{args.base}...HEAD")
    added = [
        Path(name)
        for status, name in (line.split("\t", 1) for line in changed)
        if status == "A" and name.startswith("changelog.d/")
    ]
    diff = git("diff", "--unified=0", f"{args.base}...HEAD", "--", "CHANGELOG.md")
    written = [line for line in diff if line.startswith("+") and not line.startswith("+++")]
    # Lines taken out of CHANGELOG.md are entries moving into fragments, which
    # neither conflicts nor announces a new break.
    moved = {line[1:] for line in diff if line.startswith("-") and not line.startswith("---")}

    ok = True
    if written and not args.head_ref.startswith("release-plz-"):
        error(Path("CHANGELOG.md"), None, "CHANGELOG.md is assembled by the release PR; "
              "add a fragment under changelog.d/ instead (see changelog.d/README.md)")
        ok = False

    breaking_title = bool(BREAKING_TITLE.match(args.title))
    breaking_fragment = any(
        p.name.endswith(".breaking.md")
        and (ROOT / p).read_text(encoding="utf-8").splitlines()[0] not in moved
        for p in added
    )
    if breaking_title and not breaking_fragment:
        error(Path("changelog.d"), None, "the title carries `!`, so the PR adds a "
              "`changelog.d/<slug>.breaking.md` naming what broke")
        ok = False
    if breaking_fragment and not breaking_title:
        error(Path("changelog.d"), None, "the PR adds a breaking fragment, so its title "
              "carries `!` before the colon, as in `fix(core)!: ...`")
        ok = False
    return 0 if ok else 1


def assemble(args: argparse.Namespace) -> int:
    paths = fragments()
    if not all(check_fragment(path) for path in paths):
        return 1
    if not paths:
        print("changelog.d/ holds no fragments; CHANGELOG.md is left as release-plz wrote it")
        return 0

    lines = CHANGELOG.read_text(encoding="utf-8").splitlines()
    try:
        unreleased = lines.index("## [Unreleased]")
    except ValueError:
        error(CHANGELOG, None, "no `## [Unreleased]` heading to find the new section under")
        return 1
    start = next((i for i in range(unreleased + 1, len(lines)) if lines[i].startswith("## ")), None)
    if start is None:
        error(CHANGELOG, None, "release-plz wrote no version section under [Unreleased]")
        return 1
    if any(line.strip() for line in lines[unreleased + 1:start]):
        error(CHANGELOG, unreleased + 1, "[Unreleased] has entries of its own; "
              "they belong in changelog.d/")
        return 1
    end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith("## ")), len(lines))

    # The section as release-plz wrote it: one `### Group` per commit group,
    # each followed by one title line per PR.
    sections: dict[str, list[str]] = {}
    current = None
    for line in lines[start + 1:end]:
        if line.startswith("### "):
            current = line[4:].strip()
            sections.setdefault(current, [])
        elif current is not None:
            sections[current].append(line)

    for path in paths:
        heading = KINDS[FRAGMENT_NAME.match(path.name)["kind"]]
        body = sections.setdefault(heading, [])
        while body and not body[-1].strip():
            body.pop()
        if body:
            body.append("")
        body.extend(path.read_text(encoding="utf-8").strip("\n").splitlines())

    order = list(KINDS.values()) + [h for h in sections if h not in KINDS.values()]
    rendered = [lines[start], ""]
    for heading in order:
        body = sections.get(heading)
        if body is None:
            continue
        while body and not body[0].strip():
            body.pop(0)
        while body and not body[-1].strip():
            body.pop()
        rendered += [f"### {heading}", "", *body, ""]

    CHANGELOG.write_text("\n".join(lines[:start] + rendered + lines[end:]) + "\n", encoding="utf-8")
    if not args.keep:
        for path in paths:
            path.unlink()
    print(f"folded {len(paths)} fragment(s) into {lines[start]}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("check", help="check every fragment").set_defaults(run=check)
    on_pr = commands.add_parser("pr", help="check a PR's title against its fragments")
    on_pr.add_argument("--title", required=True)
    on_pr.add_argument("--base", required=True, help="the base ref to diff against")
    on_pr.add_argument("--head-ref", default="", help="the PR's head branch name")
    on_pr.set_defaults(run=pr)
    fold = commands.add_parser("assemble", help="fold the fragments into CHANGELOG.md")
    fold.add_argument("--keep", action="store_true", help="leave the fragment files in place")
    fold.set_defaults(run=assemble)
    args = parser.parse_args()
    return args.run(args)


if __name__ == "__main__":
    sys.exit(main())
