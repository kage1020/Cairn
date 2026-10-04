# Changelog fragments

A pull request with something to tell a reader of [CHANGELOG.md](../CHANGELOG.md) adds one file
here instead of editing CHANGELOG.md. Twenty open PRs that each add an entry under the same
heading conflict with one another on every merge; twenty PRs that each add their own file never
do.

This covers the root CHANGELOG.md only. `editors/vscode/CHANGELOG.md` is the extension's own and
is still edited by hand.

## Writing one

Name the file `<slug>.<kind>.md`. The slug starts with a lowercase letter or digit and goes on in
those and `-`, and no other fragment has it; a few words of what changed is enough
(`walkway-port-cap.fixed.md`). The kind picks the heading the entry lands under:

| kind           | heading            | when                                                            |
| -------------- | ------------------ | --------------------------------------------------------------- |
| `breaking`     | `Breaking changes` | the PR title carries `!`; name every removed or retyped item    |
| `deprecations` | `Deprecations`     | a Stable surface starts warning ahead of its removal            |
| `added`        | `Added`            | a `feat` PR                                                     |
| `changed`      | `Changed`          | a `refactor` PR                                                 |
| `fixed`        | `Fixed`            | a `fix` PR                                                      |
| `performance`  | `Performance`      | a `perf` PR                                                     |
| `build`        | `Build`            | a `build` PR, such as raising `rust-version`                    |

`added` through `build` are the five groups `commit_parsers` in
[release-plz.toml](../release-plz.toml) does not skip, so the fragment lands beside the line
release-plz writes from the PR's title. `breaking` and `deprecations` are the two sections
[compatibility "How a break is communicated"](https://cairn.kage1020.com/spec/compatibility/)
requires. A PR can add more than one fragment, one per kind.

The body is the entry as it should read in CHANGELOG.md: one or more `- ` items, the first on the
file's first line, each opening with the scope when there is one. Everything else, nested lists
and code fences included, is indented two spaces under an item. No heading; the file name gives
it. Say what changed and why rather than restating the PR title, which release-plz already writes.

````markdown
- *(core)* A `place` row refused with `E_INVALID_PLACE_ID` drew a second error on every later row
  whose `east_of=` / `north_of=` named it. That row is now `W_DEFERRED_PLACE`:
  - a `connect` naming it still gets `W_DEFERRED_CONNECT`;
  - a reference above the refused row is still `E_UNRESOLVED_PLACE_REF`.

  ```text
  warning[W_DEFERRED_PLACE]: ...
  ```
````

## What CI checks

The `Changelog` workflow runs `changelog.py pr` on every PR. It reports, all at once:

- a fragment the PR adds or edits whose name is not `<slug>.<kind>.md` with a kind from the table
  (a stray file such as `.gitkeep` or a directory counts; a `README*.md` does not);
- a fragment that is empty, does not start on its first line with a `- ` item, has an item with
  nothing after its scope, or has a line that is neither an item nor indented under one;
- an issue or PR number, as `#123` or as a link to an issue or PR. Say what changed, so a reader
  need not open GitHub to learn what a line means;
- an entry scoped to `ci` alone, which has no entry of its own, as release-plz skips the group;
- a title with `!` and no `breaking` fragment added by this PR, or one added and no `!`;
- any change to CHANGELOG.md, except in the release PR, which `github-actions[bot]` opens from a
  `release-plz-*` branch. There it instead requires that every fragment was folded in.

The promote-to-main PR, which `github-actions[bot]` opens from `canary`, is exempt: everything in it was checked on its way into `canary`. Until
`Changelog` is a required check in the branch ruleset, a red result does not block the merge, and
the reviewer asks for the fix.

Run the same check before pushing, with your PR's own title:

```sh
git fetch origin canary
python3 .github/scripts/changelog.py check
python3 .github/scripts/changelog.py pr --base canary \
  --head-ref "$(git branch --show-current)" --author "$(git config user.name)" \
  --title "$(git log -1 --format=%s)"
```

`check` reads every fragment in the tree, not only yours. The script's own tests run with
`python3 -m unittest discover -s .github/scripts`.

## At release

The monthly release PR runs `changelog.py assemble --version <version> --delete` after
release-plz has written the new version's section, and refuses if the section under
`[Unreleased]` is any other version's. Each fragment goes under its heading in that section,
after the title lines release-plz wrote, in file-name order, and the fragment files are deleted in
the same commit. The headings are then ordered as the table above, `Breaking changes` first and
`Build` last, with any heading release-plz wrote that has no kind (`Other`, or a `Docs` line kept
for its `!`) after them. Before publishing, `changelog.py verify-release` refuses a version that
is not yet tagged if any fragment is left or its section is missing.

To see what a fold would do, run `assemble --version <version>` on a copy where release-plz's
section is in place; without `--delete` the fragments stay. Edit the release PR's CHANGELOG.md if
an entry needs rewording once it sits beside the others, keeping every line under a `###`
heading.
