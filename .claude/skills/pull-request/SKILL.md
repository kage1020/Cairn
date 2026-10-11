---
name: pull-request
description: Title a Cairn pull request and write its changelog fragment, when opening or retitling a PR against canary.
---

# Open a pull request

CONTRIBUTING.md "Branches and pull requests" and `changelog.d/README.md` are the full rules; this
is the sequence.

1. Branch from `canary` as `<type>/<short-kebab>`, and target `canary`. Never open a PR against
   `main`.
2. Title the PR with a Conventional Commits line. The squash merge makes it the commit on
   `canary`, and release-plz reads it.
   - Scope is the crate without its prefix or the spec area: `core`, `nbt`, `formats`,
     `redstone`, `cli`, `lsp`, `tree-sitter`, `spec`, `deps`.
   - `feat`, `fix`, `perf`, `refactor` and `build` cut a patch release; `docs`, `test`, `ci`,
     `chore` and `style` do not.
   - A breaking change puts `!` before the colon and adds a `breaking` fragment. The two always
     come together.
3. Add one fragment per kind as `changelog.d/<slug>.<kind>.md`. The kind follows the type:

   | type | kind |
   |---|---|
   | `feat` | `added` |
   | `fix` | `fixed` |
   | `refactor` | `changed` |
   | `perf` | `performance` |
   | `build` | `build` |
   | any, with `!` | `breaking`, naming every removed or retyped public item |

   `docs`, `test`, `ci`, `chore` and `style` changes get no fragment. Write each entry as
   `- *(scope)* ...` lines that say what changed for a user and why, without issue or PR numbers.
   Never edit `CHANGELOG.md`.
4. Fill in the sections of `.github/pull_request_template.md` for the body.
5. Check the fragments and the title together before pushing:

   ```sh
   git fetch origin canary
   python3 .github/scripts/changelog.py check
   python3 .github/scripts/changelog.py pr --base canary \
     --head-ref "$(git branch --show-current)" --author "$(git config user.name)" \
     --title "<the PR title>"
   ```
