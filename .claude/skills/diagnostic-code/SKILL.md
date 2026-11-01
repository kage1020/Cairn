---
name: diagnostic-code
description: Add, rename or reclassify a Cairn diagnostic code (E_* / W_*), keeping core's enum, the severity rule, the spec catalog in both languages and the tests in step.
---

# Add a diagnostic code

A code's string is public contract: `--format json` consumers branch on it. Its spec row is where
a reader with the code in hand looks it up. Both land in the same pull request as the code.

1. **Spec.** Add a row to the right table under "Diagnostic codes" in `spec/lint`, in
   `website/src/content/docs/spec/lint.md` and `website/src/content/docs/ja/spec/lint.md`. When the
   rule belongs to another chapter, the row says what the code means and links there. When the
   code's severity does not follow from its `E_` / `W_` prefix, say why under "Error vs warning".
   The redstone pipeline's codes (`crates/cairn-lang-redstone/src/diagnostic.rs`: `E_LOGIC_*` /
   `W_LOGIC_*`, `E_NO_CIRCUIT_REGION`, `E_ROUTE_CONGESTION`, `E_ATTENUATION_LIMIT`,
   `W_ROUTE_CROSS_LAYER_CLEARANCE`) are Internal tier and get no row. A code the CLI prints itself,
   as `error[E_…]`, `warning[W_…]` or `W_…:`, gets a row like core's, and skips step 2.
2. **Enum.** In `crates/cairn-lang-core/src/diagnostic.rs`:
   - add the variant to `DiagnosticCode` with a one-line doc of what it reports;
   - add its string to `as_str` and its severity to `severity`. The severity follows `spec/lint`
     "Error vs warning", not the prefix;
   - add a `DiagnosticData` variant only when a consumer needs a field it would otherwise have to
     parse out of the message.
3. **Emit.** Build the `Diagnostic` in the pass that owns the rule. The `primary` message says what
   is wrong, the notes say what would be valid and how to fix it, and a note with a span points at
   the other location involved (the first declaration, for a duplicate). For an unknown name, find
   a near spelling with `suggest::nearest_match` and attach it with `suggest::did_you_mean_note`.
4. **Tests.**
   - `crates/cairn-lang-core/src/diagnostic/tests.rs` lists every code's string and its severity
     partition. Add the new string to both lists.
   - Pin the finding in the pass's integration tests, under `crates/cairn-lang-core/tests/`: the
     exact codes, spans and messages for one source that raises it and one near miss that does not.
   - An `Error` raised by `check` also gets a fixture in
     `crates/cairn-lang-cli/tests/cli/check/parity.rs`, which shows every build command refuses it.
5. Run `cargo xtask lint diagnostic-catalog`, then the `verify` skill.

Renaming a code or moving it across the error/warning line breaks consumers. The changelog
fragment for it is `breaking` (see the `pull-request` skill).
