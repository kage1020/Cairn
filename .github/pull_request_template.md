<!--
PR title MUST be a Conventional Commits line. The title becomes the squash-merge
commit message on `main` and is parsed by release-plz to decide patch releases.

  feat(core): add lexer
  fix(formats): correct big-endian NBT length
  docs(spec): clarify §6.3 derivation order
  feat(redstone)!: rewrite tick simulator       (breaking — see Compatibility C.3)
  build(deps): bump tower to 0.5
  refactor(nbt): split codec into modules
  ci: add windows-11-arm to publish matrix

Types that trigger a patch release:    feat, fix, perf, refactor, build
Types that do NOT:                     docs, test, ci, chore, style

See CONTRIBUTING.md for the full convention.
-->

## Summary

<!-- One paragraph: what does this change, and why? -->

## Linked issues / spec sections

<!-- e.g. closes #123, refs spec §6.3, refs roadmap M2 -->

## Test plan

<!-- How was this verified? cargo test? manual run? n/a for docs-only? -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] (if docs) `pnpm build` in `website/`

## Compatibility

<!--
If this touches a Stable surface (see spec/compatibility):
  - Did you add a `W_DEPRECATED` warning for the previous release? (link the PR)
  - Did you update CHANGELOG `Deprecations` / `Breaking changes` accordingly?
If this is `feat!:` / `fix!:` etc., explain the migration path.
Delete this section if the change is purely internal.
-->
