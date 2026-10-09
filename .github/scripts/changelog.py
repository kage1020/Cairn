#!/usr/bin/env python3

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FRAGMENT_DIR = ROOT / "changelog.d"
CHANGELOG = ROOT / "CHANGELOG.md"

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
# READMEs explain the directory; every other file in it is a fragment.
README_NAME = re.compile(r"^README(\.[a-z]+)?\.md$")
BREAKING_TITLE = re.compile(r"^[a-z]+(\([^)]*\))?!:")
# The `&` keeps a numeric HTML entity such as `&#8217;` from reading as one.
BARE_NUMBER = re.compile(r"(?<![\w&])#\d+\b")
GITHUB_REFERENCE = re.compile(r"github\.com/[\w.-]+/[\w.-]+/(issues|pull)/\d+")
CI_ONLY_SCOPE = re.compile(r"^- \*\(ci\)\*")
SCOPE = re.compile(r"^\*\([^)]*\)\*")

RELEASE_BOT = "github-actions[bot]"
RELEASE_BRANCH_PREFIX = "release-plz-"


class Refused(Exception):
    """Something outside the fragments themselves stops the command."""


def error(path: Path | str, line: int | None, message: str) -> None:
    path = Path(path)
    shown = path.relative_to(ROOT) if path.is_absolute() else path
    if os.environ.get("GITHUB_ACTIONS") == "true":
        where = f"file={shown}" + (f",line={line}" if line else "")
        print(f"::error {where}::{message}")
    else:
        print(f"{shown}{f':{line}' if line else ''}: {message}", file=sys.stderr)


def fragments() -> list[Path]:
    if not FRAGMENT_DIR.is_dir():
        raise Refused("changelog.d/ is missing; without it no fragment can be checked or folded")
    return sorted(p for p in FRAGMENT_DIR.iterdir() if not README_NAME.match(p.name))


def check_fragment(path: Path) -> bool:
    """Report what is wrong with one fragment; true when nothing is."""
    match = FRAGMENT_NAME.match(path.name)
    if not path.is_file() or not match:
        error(path, None, "a fragment is named `<slug>.<kind>.md`: the slug starts with a "
              "lowercase letter or digit and goes on in those and `-`, the kind is one of "
              + ", ".join(KINDS))
        return False
    if match["kind"] not in KINDS:
        error(path, None, f"unknown kind `{match['kind']}`; one of " + ", ".join(KINDS))
        return False

    lines = path.read_text(encoding="utf-8").splitlines()
    if not any(line.strip() for line in lines):
        error(path, None, "the fragment is empty")
        return False
    ok = True
    if not lines[0].startswith("- "):
        error(path, 1, "a fragment starts on its first line with a `- ` list item")
        ok = False
    for number, line in enumerate(lines, start=1):
        if line.strip() and not (line.startswith("- ") or line.startswith("  ")):
            error(path, number, "every line is a `- ` item or indented under one, code "
                  "fences included; the heading comes from the file name")
            ok = False
        if line.startswith("- ") and not SCOPE.sub("", line[2:]).strip():
            error(path, number, "the item says nothing; write what changed")
            ok = False
        if BARE_NUMBER.search(line) or GITHUB_REFERENCE.search(line):
            error(path, number, "no issue or PR number or link in the tree: say what "
                  "changed, so a reader need not open GitHub to learn it")
            ok = False
        if CI_ONLY_SCOPE.match(line):
            error(path, number, "a `ci` change has no entry of its own; release-plz "
                  "skips the `ci` group too")
            ok = False
    return ok


def check(_: argparse.Namespace) -> int:
    results = [check_fragment(path) for path in fragments()]
    return 0 if all(results) else 1


def git(*args: str, ok_codes: tuple[int, ...] = (0,)) -> subprocess.CompletedProcess[str]:
    out = subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True,
                         encoding="utf-8", errors="replace")
    if out.returncode not in ok_codes:
        raise Refused(f"`git {' '.join(args)}` failed, which is not about this PR's "
                      f"changelog: {out.stderr.strip()}")
    return out


def workspace_version() -> str:
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    return manifest["workspace"]["package"]["version"]


def release_problems() -> list[str]:
    """What stops CHANGELOG.md and changelog.d/ from being ready to publish."""
    version = workspace_version()
    problems = []
    left = fragments()
    if left:
        problems.append(f"{len(left)} fragment(s) were never folded in, so {version} would "
                        "ship without them: " + ", ".join(p.name for p in left))
    lines = CHANGELOG.read_text(encoding="utf-8").splitlines()
    heading = next((line for line in lines if line.startswith("## ") and line != "## [Unreleased]"),
                   None)
    if heading is None or not heading.startswith(f"## {version} "):
        problems.append(f"the first section under [Unreleased] is `{heading}`, not {version}'s")
    return problems


def pr(args: argparse.Namespace) -> int:
    if args.base == "main" and args.head_ref == "canary" and args.author == RELEASE_BOT:
        print("promote-to-main PR: nothing here was not already checked on canary")
        return 0

    base = f"origin/{args.base}"
    changed = git("diff", "--name-status", "--no-renames", f"{base}...HEAD").stdout.splitlines()
    status = {name: code for code, name in (line.split("\t", 1) for line in changed)}
    release_pr = args.author == RELEASE_BOT and args.head_ref.startswith(RELEASE_BRANCH_PREFIX)
    bootstrap = git("cat-file", "-e", f"{base}:changelog.d/README.md",
                    ok_codes=(0, 128)).returncode != 0

    if release_pr:
        problems = release_problems()
        for problem in problems:
            error(CHANGELOG, None, problem)
        return 1 if problems else 0

    touched = [
        ROOT / name for name, code in status.items()
        if code in "AM" and name.startswith("changelog.d/")
        and not README_NAME.match(Path(name).name)
    ]
    ok = all([check_fragment(path) for path in touched])

    if "CHANGELOG.md" in status and not bootstrap:
        error(CHANGELOG, None, "CHANGELOG.md is assembled by the release PR; add a fragment "
              "under changelog.d/ instead (see changelog.d/README.md)")
        ok = False

    if not bootstrap:
        breaking_title = bool(BREAKING_TITLE.match(args.title))
        breaking_fragment = any(
            status[name] == "A" and name.startswith("changelog.d/") and name.endswith(".breaking.md")
            for name in status
        )
        if breaking_title and not breaking_fragment:
            error("changelog.d", None, "the title carries `!`, so the PR adds a "
                  "`changelog.d/<slug>.breaking.md` naming what broke")
            ok = False
        if breaking_fragment and not breaking_title:
            error("changelog.d", None, "the PR adds a breaking fragment, so its title "
                  "carries `!` before the colon, as in `fix(core)!: ...`")
            ok = False
    return 0 if ok else 1


def assemble(args: argparse.Namespace) -> int:
    paths = fragments()
    if not all([check_fragment(path) for path in paths]):
        return 1

    lines = CHANGELOG.read_text(encoding="utf-8").splitlines()
    try:
        unreleased = lines.index("## [Unreleased]")
    except ValueError:
        error(CHANGELOG, None, "no `## [Unreleased]` heading to find the new section under")
        return 1
    start = next((i for i in range(unreleased + 1, len(lines)) if lines[i].startswith("## ")), None)
    if start is None or not lines[start].startswith(f"## {args.version} "):
        found = f"`{lines[start]}`" if start is not None else "nothing"
        error(CHANGELOG, (start or unreleased) + 1, f"the section under [Unreleased] is {found}, "
              f"not {args.version}'s; release-plz writes that section before this runs")
        return 1
    if any(line.strip() for line in lines[unreleased + 1:start]):
        error(CHANGELOG, unreleased + 1, "[Unreleased] has entries of its own; "
              "they belong in changelog.d/")
        return 1
    end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith("## ")), len(lines))

    sections: dict[str, list[str]] = {}
    current = None
    for number, line in enumerate(lines[start + 1:end], start=start + 2):
        if line.startswith("### "):
            current = line[4:].strip()
            sections.setdefault(current, [])
        elif current is not None:
            sections[current].append(line)
        elif line.strip():
            error(CHANGELOG, number, "a line under the version heading but above its first "
                  "`###` heading; move it under one, or it would be lost")
            return 1

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

    CHANGELOG.write_text("\n".join(lines[:start] + rendered + lines[end:]) + "\n",
                         encoding="utf-8", newline="\n")
    if args.delete:
        for path in paths:
            path.unlink()
    kept = "" if args.delete or not paths else "; the fragment files are kept (pass --delete)"
    print(f"folded {len(paths)} fragment(s) into {lines[start]}{kept}")
    return 0


def verify_release(_: argparse.Namespace) -> int:
    version = workspace_version()
    if git("tag", "--list", f"v{version}").stdout.strip():
        print(f"v{version} is already tagged; nothing is about to be released")
        return 0
    problems = release_problems()
    for problem in problems:
        error(CHANGELOG, None, problem)
    return 1 if problems else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("check", help="check every fragment").set_defaults(run=check)
    on_pr = commands.add_parser("pr", help="what CI checks on a pull request")
    on_pr.add_argument("--base", required=True, help="the PR's base branch, such as canary")
    on_pr.add_argument("--head-ref", required=True, help="the PR's head branch")
    on_pr.add_argument("--author", required=True, help="the login that opened the PR")
    on_pr.add_argument("--title", required=True)
    on_pr.set_defaults(run=pr)
    fold = commands.add_parser("assemble", help="fold the fragments into CHANGELOG.md")
    fold.add_argument("--version", required=True, help="the version release-plz wrote a section for")
    fold.add_argument("--delete", action="store_true", help="delete the fragments once folded")
    fold.set_defaults(run=assemble)
    commands.add_parser("verify-release", help="refuse to publish without the folded entry") \
        .set_defaults(run=verify_release)
    args = parser.parse_args()
    try:
        return args.run(args)
    except Refused as refused:
        error("changelog.d", None, str(refused))
        return 1


if __name__ == "__main__":
    sys.exit(main())
