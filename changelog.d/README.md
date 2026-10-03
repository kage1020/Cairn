# Changelog fragments

A pull request with something to tell a reader of [CHANGELOG.md](../CHANGELOG.md) adds one file
here instead of editing CHANGELOG.md. Twenty open PRs that each add an entry under the same
heading conflict with one another on every merge; twenty PRs that each add their own file never
do.

## Writing one

Name the file `<slug>.<kind>.md`. The slug is any name of lowercase letters, digits and `-` that
no other fragment has; a few words of what changed is enough (`walkway-port-cap.fixed.md`). The
kind picks the heading the entry lands under:

| kind           | heading            | when                                                            |
| -------------- | ------------------ | --------------------------------------------------------------- |
| `breaking`     | `Breaking changes` | the PR title carries `!`; name every removed or retyped item    |
| `deprecations` | `Deprecations`     | a Stable surface starts warning ahead of its removal            |
| `added`        | `Added`            | a `feat` PR                                                     |
| `changed`      | `Changed`          | a `refactor` PR                                                 |
| `fixed`        | `Fixed`            | a `fix` PR                                                      |
| `performance`  | `Performance`      | a `perf` PR                                                     |
| `build`        | `Build`            | a `build` PR, such as raising `rust-version`                    |

`added` through `build` are the groups `commit_parsers` in [release-plz.toml](../release-plz.toml)
gives each commit type, so the fragment lands beside the line release-plz writes from the PR's
title. `breaking` and `deprecations` are the two sections
[compatibility "How a break is communicated"](https://cairn.kage1020.com/spec/compatibility/)
requires. A PR can add more than one fragment, one per kind.

The body is the entry as it should read in CHANGELOG.md: a `- ` item, opening with the scope when
there is one, and continuation lines indented two spaces. No heading; the file name gives it.

```markdown
- *(core)* A `place` row refused with `E_INVALID_PLACE_ID` drew a second error on every later row
  whose `east_of=` / `north_of=` named it. That row is now `W_DEFERRED_PLACE`.
```

## What CI holds you to

The `Changelog` workflow runs `python3 .github/scripts/changelog.py check` and `... pr`, which
refuse:

- a file here whose name is not `<slug>.<kind>.md` with a kind from the table;
- a line that is neither a `- ` item nor indented under one;
- an issue or PR number. Say what changed, so a reader need not open GitHub to learn what a line
  means; a `[#123](https://github.com/kage1020/Cairn/pull/123)` link is the one form allowed;
- an entry scoped to `ci` alone, which has no entry of its own, as release-plz skips the group;
- a title with `!` and no new `breaking` fragment, or a new `breaking` fragment and no `!`;
- a line added to CHANGELOG.md by any PR but the release PR.

Run the same checks before pushing:

```sh
python3 .github/scripts/changelog.py check
python3 .github/scripts/changelog.py pr --base origin/canary --title 'fix(core)!: ...'
```

## At release

The monthly release PR runs `changelog.py assemble` after release-plz has written the new
version's section. Each fragment goes under its heading in that section, after the title lines
release-plz wrote, in file-name order, and the fragment files are deleted in the same commit. The
section's headings are ordered `Breaking changes`, `Deprecations`, then the commit groups. Edit
the release PR's CHANGELOG.md if an entry needs rewording once it sits beside the others.
