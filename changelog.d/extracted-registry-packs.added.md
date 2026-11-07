- *(formats)* The built-in registry packs are extracted from the game: Java's block tables from
  each version's server jar `--reports`, Bedrock's from the release's block palette as GeyserMC
  ships it, every state the game accepts. Each block now carries its state properties and the
  values each takes (Java's as strings, Bedrock's typed as booleans, integers and strings), and
  `BlocksIndex` answers `states_of`, `since` and `until`, the first and last carried version that
  declares an ID. `pack.json` records the file each table came from with its hash, and
  `.github/scripts/extract_registry.py` regenerates the tables or, with `--check`, fails when a
  committed one differs from the game.
- *(formats)* Java 26.3 and Bedrock 1.26.60 are targets, and `--target latest` resolves to them. A
  26.3 `.nbt` names each palette entry's block and properties `id` and `properties`, the keys that
  version reads, and every example loads in the 26.3 server unchanged. `chain`, which both releases
  spell `iron_chain`, is answered with the new name.
