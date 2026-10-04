"""Tests for changelog.py, run as `python3 -m unittest discover -s .github/scripts`.

Each test copies the script into a scratch git repository and runs it there as
a subprocess, so `ROOT` is that repository and nothing touches this one.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("changelog.py")

CHANGELOG = """\
# Changelog

## [Unreleased]

## 2026.10.0 — 2026-10-01

### Fixed
- *(core)* an old fix
"""

RELEASE_SECTION = """\
## 2026.11.0 — 2026-11-01

### Added
- *(core)* a feature title line

### Build
- *(tree-sitter)* a build title line

### Fixed
- *(core)* a fix title line

### Other
- an untyped title line

"""

GOOD = "- *(core)* A thing was wrong. It is right now.\n"


class Repo:
    """A scratch repository with the script, a manifest and a changelog."""

    def __init__(self, version: str = "2026.10.0") -> None:
        self.dir = Path(tempfile.mkdtemp())
        (self.dir / ".github/scripts").mkdir(parents=True)
        shutil.copy(SCRIPT, self.dir / ".github/scripts/changelog.py")
        (self.dir / "changelog.d").mkdir()
        self.write("changelog.d/README.md", "# Changelog fragments\n")
        self.write("CHANGELOG.md", CHANGELOG)
        self.manifest(version)
        self.git("init", "-q", "-b", "canary")
        self.commit("base")
        self.git("update-ref", "refs/remotes/origin/canary", "HEAD")
        self.git("checkout", "-q", "-b", "topic")

    def manifest(self, version: str) -> None:
        self.write("Cargo.toml", f'[workspace]\n\n[workspace.package]\nversion = "{version}"\n')

    def write(self, name: str, text: str) -> None:
        (self.dir / name).write_text(text, encoding="utf-8")

    def read(self, name: str) -> str:
        return (self.dir / name).read_text(encoding="utf-8")

    def git(self, *args: str) -> None:
        subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", *args],
                       cwd=self.dir, check=True, capture_output=True)

    def commit(self, message: str) -> None:
        self.git("add", "-A")
        self.git("commit", "-q", "--allow-empty", "-m", message)

    def run(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run([sys.executable, ".github/scripts/changelog.py", *args],
                              cwd=self.dir, capture_output=True, text=True, encoding="utf-8",
                              env={"PATH": "/usr/bin:/bin", "GITHUB_ACTIONS": "false"})

    def pr(self, title: str, head: str = "topic", base: str = "canary",
           author: str = "someone") -> subprocess.CompletedProcess[str]:
        self.commit("change")
        return self.run("pr", "--base", base, "--head-ref", head, "--author", author,
                        "--title", title)

    def cleanup(self) -> None:
        shutil.rmtree(self.dir, ignore_errors=True)


class Case(unittest.TestCase):
    def setUp(self) -> None:
        self.fresh()

    def fresh(self) -> None:
        """Start over in a new scratch repository."""
        self.repo = Repo()
        self.addCleanup(self.repo.cleanup)

    def assertRefused(self, result: subprocess.CompletedProcess[str], says: str) -> None:
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(says, result.stdout + result.stderr)

    def assertPasses(self, result: subprocess.CompletedProcess[str]) -> None:
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


class CheckTest(Case):
    def check_one(self, name: str, text: str) -> subprocess.CompletedProcess[str]:
        self.repo.write(f"changelog.d/{name}", text)
        return self.repo.run("check")

    def test_a_well_formed_fragment_with_a_nested_list_and_a_fence_passes(self) -> None:
        self.assertPasses(self.check_one("thing.fixed.md", GOOD + (
            "  - one detail\n"
            "  ```text\n"
            "  cairn check\n"
            "  ```\n"
            "\n"
            "  A second paragraph.\n")))

    def test_an_html_entity_is_not_a_number(self) -> None:
        self.assertPasses(self.check_one("thing.fixed.md", "- *(core)* it&#8217;s fixed\n"))

    def test_a_translated_readme_is_not_a_fragment(self) -> None:
        self.assertPasses(self.check_one("README.ja.md", "# 説明\n"))

    def test_bad_names_and_kinds(self) -> None:
        self.assertRefused(self.check_one("Bad.fixed.md", GOOD), "is named `<slug>.<kind>.md`")
        self.fresh()
        self.assertRefused(self.check_one("-x.fixed.md", GOOD), "is named `<slug>.<kind>.md`")
        self.fresh()
        self.assertRefused(self.check_one("x.fix.md", GOOD), "unknown kind `fix`")
        self.fresh()
        self.assertRefused(self.check_one(".gitkeep", ""), "is named `<slug>.<kind>.md`")

    def test_a_leading_blank_line_is_refused(self) -> None:
        self.assertRefused(self.check_one("x.breaking.md", "\n" + GOOD), "on its first line")

    def test_an_empty_fragment_or_item_is_refused(self) -> None:
        self.assertRefused(self.check_one("x.fixed.md", "\n"), "the fragment is empty")
        self.fresh()
        self.assertRefused(self.check_one("x.fixed.md", "- *(core)* \n"), "the item says nothing")

    def test_numbers_and_links_are_refused(self) -> None:
        for text in ["- *(core)* fixed (#454)\n",
                     "- *(core)* fixed ([#455](https://github.com/kage1020/Cairn/pull/455))\n",
                     "- *(core)* see https://github.com/kage1020/Cairn/issues/12\n"]:
            with self.subTest(text=text):
                self.assertRefused(self.check_one("x.fixed.md", text), "no issue or PR number")

    def test_an_unindented_line_is_refused(self) -> None:
        self.assertRefused(self.check_one("x.fixed.md", GOOD + "```\ncode\n```\n"),
                           "indented under one")

    def test_a_ci_only_entry_is_refused(self) -> None:
        self.assertRefused(self.check_one("x.fixed.md", "- *(ci)* a workflow\n"), "`ci` change")

    def test_a_missing_directory_is_refused(self) -> None:
        shutil.rmtree(self.repo.dir / "changelog.d")
        self.assertRefused(self.repo.run("check"), "changelog.d/ is missing")


class PrTest(Case):
    def test_a_breaking_title_needs_a_new_breaking_fragment(self) -> None:
        self.repo.write("changelog.d/x.fixed.md", GOOD)
        self.assertRefused(self.repo.pr("fix(core)!: x"), "the title carries `!`")

    def test_a_breaking_fragment_needs_a_breaking_title(self) -> None:
        self.repo.write("changelog.d/x.breaking.md", GOOD)
        self.assertRefused(self.repo.pr("fix(core): x"), "its title carries `!`")

    def test_a_breaking_title_and_fragment_together_pass(self) -> None:
        self.repo.write("changelog.d/x.breaking.md", GOOD)
        self.assertPasses(self.repo.pr("fix(core)!: x"))

    def test_only_the_fragments_the_pr_touches_are_checked(self) -> None:
        self.repo.git("checkout", "-q", "canary")
        self.repo.write("changelog.d/Broken.fixed.md", GOOD)
        self.repo.commit("a broken fragment already on canary")
        self.repo.git("update-ref", "refs/remotes/origin/canary", "HEAD")
        self.repo.git("checkout", "-q", "-b", "other")
        self.repo.write("changelog.d/y.fixed.md", GOOD)
        self.assertPasses(self.repo.pr("fix(core): y", head="other"))

    def test_all_problems_are_reported_together(self) -> None:
        self.repo.write("changelog.d/x.breaking.md", "- #12\n")
        result = self.repo.pr("fix(core): x")
        self.assertRefused(result, "no issue or PR number")
        self.assertIn("its title carries `!`", result.stderr)

    def test_changing_the_changelog_is_refused_either_way(self) -> None:
        self.repo.write("CHANGELOG.md", CHANGELOG + "- added\n")
        self.assertRefused(self.repo.pr("fix: x"), "assembled by the release PR")
        self.fresh()
        self.repo.write("CHANGELOG.md", CHANGELOG.replace("- *(core)* an old fix\n", ""))
        self.assertRefused(self.repo.pr("fix: x"), "assembled by the release PR")

    def test_a_branch_named_like_the_release_pr_is_not_the_release_pr(self) -> None:
        self.repo.write("CHANGELOG.md", CHANGELOG + "- added\n")
        self.assertRefused(self.repo.pr("fix: x", head="release-plz-fix"), "assembled by")

    def test_the_release_pr_must_have_folded_every_fragment(self) -> None:
        self.repo.manifest("2026.11.0")
        self.repo.write("CHANGELOG.md", CHANGELOG.replace("## 2026.10.0", RELEASE_SECTION
                                                          + "## 2026.10.0"))
        self.repo.write("changelog.d/left.fixed.md", GOOD)
        self.assertRefused(self.repo.pr("chore: release v2026.11.0", head="release-plz-x",
                                        author="github-actions[bot]"), "never folded in")

    def test_the_release_pr_must_carry_its_own_section(self) -> None:
        self.repo.manifest("2026.11.0")
        self.assertRefused(self.repo.pr("chore: release v2026.11.0", head="release-plz-x",
                                        author="github-actions[bot]"), "not 2026.11.0's")

    def test_a_folded_release_pr_passes(self) -> None:
        self.repo.manifest("2026.11.0")
        self.repo.write("CHANGELOG.md", CHANGELOG.replace("## 2026.10.0", RELEASE_SECTION
                                                          + "## 2026.10.0"))
        self.assertPasses(self.repo.pr("chore: release v2026.11.0", head="release-plz-x",
                                       author="github-actions[bot]"))

    def test_the_promote_pr_passes(self) -> None:
        self.repo.git("update-ref", "refs/remotes/origin/main", "HEAD")
        self.repo.write("CHANGELOG.md", CHANGELOG.replace("## 2026.10.0", RELEASE_SECTION
                                                          + "## 2026.10.0"))
        self.repo.write("changelog.d/x.breaking.md", GOOD)
        self.assertPasses(self.repo.pr("chore: promote canary to main for v2026.11.0",
                                       head="canary", base="main", author="github-actions[bot]"))
        self.assertRefused(self.repo.pr("chore: promote canary to main for v2026.11.0",
                                        head="canary", base="main"), "assembled by")

    def test_the_pr_that_introduces_the_directory_may_move_entries(self) -> None:
        self.repo.git("checkout", "-q", "canary")
        self.repo.git("rm", "-q", "-r", "changelog.d")
        self.repo.commit("before changelog.d")
        self.repo.git("update-ref", "refs/remotes/origin/canary", "HEAD")
        self.repo.git("checkout", "-q", "-b", "introduce")
        (self.repo.dir / "changelog.d").mkdir()
        self.repo.write("changelog.d/README.md", "# Changelog fragments\n")
        self.repo.write("changelog.d/moved.breaking.md", GOOD)
        self.repo.write("CHANGELOG.md", CHANGELOG.replace("- *(core)* an old fix\n", ""))
        self.assertPasses(self.repo.pr("ci: x", head="introduce"))

    def test_a_git_failure_is_one_line_not_a_traceback(self) -> None:
        result = self.repo.pr("fix: x", base="no-such-branch")
        self.assertRefused(result, "failed, which is not about this PR's changelog")
        self.assertNotIn("Traceback", result.stderr)


class AssembleTest(Case):
    def setUp(self) -> None:
        super().setUp()
        self.repo.write("CHANGELOG.md", CHANGELOG.replace("## 2026.10.0", RELEASE_SECTION
                                                          + "## 2026.10.0"))

    def section(self) -> str:
        text = self.repo.read("CHANGELOG.md")
        return text[text.index("## 2026.11.0"):text.index("## 2026.10.0")]

    def test_fragments_land_under_their_headings_in_order(self) -> None:
        self.repo.write("changelog.d/b.fixed.md", "- *(core)* second fix\n")
        self.repo.write("changelog.d/a.fixed.md", "- *(core)* first fix\n")
        self.repo.write("changelog.d/x.breaking.md", "- *(core)* a break\n")
        self.assertPasses(self.repo.run("assemble", "--version", "2026.11.0"))
        self.assertEqual(self.section(), textwrap.dedent("""\
            ## 2026.11.0 — 2026-11-01

            ### Breaking changes

            - *(core)* a break

            ### Added

            - *(core)* a feature title line

            ### Fixed

            - *(core)* a fix title line

            - *(core)* first fix

            - *(core)* second fix

            ### Build

            - *(tree-sitter)* a build title line

            ### Other

            - an untyped title line

            """))

    def test_fragments_are_kept_unless_deletion_is_asked_for(self) -> None:
        self.repo.write("changelog.d/a.fixed.md", GOOD)
        self.assertPasses(self.repo.run("assemble", "--version", "2026.11.0"))
        self.assertTrue((self.repo.dir / "changelog.d/a.fixed.md").exists())
        self.repo.write("CHANGELOG.md", CHANGELOG.replace("## 2026.10.0", RELEASE_SECTION
                                                          + "## 2026.10.0"))
        self.assertPasses(self.repo.run("assemble", "--version", "2026.11.0", "--delete"))
        self.assertFalse((self.repo.dir / "changelog.d/a.fixed.md").exists())
        self.assertTrue((self.repo.dir / "changelog.d/README.md").exists())

    def test_headings_are_ordered_the_same_with_no_fragments(self) -> None:
        self.assertPasses(self.repo.run("assemble", "--version", "2026.11.0"))
        headings = [l for l in self.section().splitlines() if l.startswith("### ")]
        self.assertEqual(headings, ["### Added", "### Fixed", "### Build", "### Other"])

    def test_another_versions_section_is_refused(self) -> None:
        self.repo.write("CHANGELOG.md", CHANGELOG)
        self.repo.write("changelog.d/a.fixed.md", GOOD)
        self.assertRefused(self.repo.run("assemble", "--version", "2026.11.0", "--delete"),
                           "not 2026.11.0's")
        self.assertEqual(self.repo.read("CHANGELOG.md"), CHANGELOG)
        self.assertTrue((self.repo.dir / "changelog.d/a.fixed.md").exists())

    def test_a_line_above_the_first_heading_is_refused(self) -> None:
        self.repo.write("CHANGELOG.md", self.repo.read("CHANGELOG.md").replace(
            "2026-11-01\n\n", "2026-11-01\n\nA note.\n\n"))
        self.assertRefused(self.repo.run("assemble", "--version", "2026.11.0"), "would be lost")

    def test_entries_left_under_unreleased_are_refused(self) -> None:
        self.repo.write("CHANGELOG.md", self.repo.read("CHANGELOG.md").replace(
            "## [Unreleased]\n", "## [Unreleased]\n\n- stray\n"))
        self.assertRefused(self.repo.run("assemble", "--version", "2026.11.0"), "entries of its own")

    def test_the_file_keeps_lf_line_endings(self) -> None:
        self.repo.write("changelog.d/a.fixed.md", GOOD)
        self.assertPasses(self.repo.run("assemble", "--version", "2026.11.0"))
        self.assertNotIn(b"\r\n", (self.repo.dir / "CHANGELOG.md").read_bytes())


class VerifyReleaseTest(Case):
    def test_a_tagged_version_passes_with_fragments_waiting(self) -> None:
        self.repo.git("tag", "v2026.10.0")
        self.repo.write("changelog.d/a.fixed.md", GOOD)
        self.assertPasses(self.repo.run("verify-release"))

    def test_an_untagged_version_needs_its_section_and_no_fragments(self) -> None:
        self.repo.manifest("2026.11.0")
        self.repo.write("changelog.d/a.fixed.md", GOOD)
        result = self.repo.run("verify-release")
        self.assertRefused(result, "never folded in")
        self.assertIn("not 2026.11.0's", result.stderr)


if __name__ == "__main__":
    unittest.main()
