- *(core,tree-sitter)* Six parser refusals pointed past the text that was wrong or named the wrong
  problem. Each now points at the text and names the mistake:
  - A bad truth-table output (`{ 0 -> 7 }`) is reported at the output, not at the `}` or the row
    after it, and a misspelled `assert` form (`assert truht(…)`) at the form rather than at
    `assert`.
  - An unknown directive with no value (`@crain`) is ``unknown directive `@crain` `` rather than
    "requires a value", which is now said only of the three directives that exist.
  - `@intended_targets` quotes the offending value as written and points at it, rather than
    printing Rust `Debug` output at the start of the value:
    ``@intended_targets expects a string such as `"1.21.4"`, got `oak` ``.
  - A `requires version>=X` under a member of a `struct` or `site` gets the `@requires` repair; the
    "goes at the body's own level" repair is for a member of a `def`, where it applies. A floor
    with an indented line under it is refused as a floor rather than read as a member whose `>=` is
    then unexpected.
  - `size=9x` in an item header, a `[…]` list or a `theme` binding, the forms where every argument
    needs its `=`, is ``size literal `9x` has no height; a size is two extents, as in `9x7` ``
    rather than ``expected `=` ``. A member command's own arguments read a key with no `=` as a
    positional value, so `window size=9x` reaches the checks it always did. The lexer's split of
    `9x` is unchanged.
  - `and` and `or` are no longer read as a signal's name where a `logic` expression expects a
    signal, so `logic sig.x = sig.a and or` is refused at the parse with
    ``expected a signal after `and`, got the operator `or` ``. Such an operand could never be read:
    `spec/redstone` "Signal names" puts every readable signal in the `sig.` namespace, and a signal
    named `or` (`-> sig.or`, read as `sig.or`) is untouched. But nothing before synthesis said so:
    `cairn check`, `cairn info` and `cairn compile` exited 0 on such a file, and `compile` wrote its
    artifacts and lockfile. That file now exits 1. The tree-sitter grammar refuses it too. A dotted
    name may still end in one (`c.or`).
