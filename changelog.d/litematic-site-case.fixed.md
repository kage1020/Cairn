- *(cli)* Under `--structure-format litematic`, two sites, or a site and a struct, whose names
  differ only in case were compared as the file system in `--out` compares them, so they built on
  Linux and were refused on macOS and Windows. The names are now folded to lower case on every
  host, and the refusal says the two differ only in case and how to fix it, as
  `E_OUTPUT_NAME_COLLISION` does for structs.
