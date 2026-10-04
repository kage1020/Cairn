- *(cli)* `cairn synth` checked for a missing `--edition` only after the source had been parsed,
  checked and synthesised, so a source with an error reported that error with exit 1 and never
  reached the usage error, and one with only warnings printed them ahead of it. The check now runs
  before the file is read, as the refusal of a stray `--edition` on `logic` / `netlist` already
  did: `--stage placement` without `--edition` exits 2 with the flag message alone, whatever the
  source holds. A path that names nothing is reported only once the flag is right, so a caller who
  got both wrong hears about the flag first and about the path on the next run.
