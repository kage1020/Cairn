- *(redstone)* `E_LOGIC_UNSUPPORTED_PRIMITIVE` says the combinational lowering has no gate for the
  primitive, and its Fix line asks only for the expression to be rewritten with `and`, `or` and
  `not`, without pointing at a later release.
- *(cli)* `-h` prints one line for each subcommand and flag, and `cairn --help` lists the
  subcommands by that line. `cairn <subcommand> --help` prints the whole text as before, exit codes
  included.
