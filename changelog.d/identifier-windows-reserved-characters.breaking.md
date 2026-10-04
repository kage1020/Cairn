- *(core)* An identifier carrying `*`, `|`, `?`, `<`, `>`, `"` or a control character is now
  refused on every host, as `.`, `:`, `/`, `\` and whitespace (a tab among them) already were. A
  `place id=` carrying one of the newly refused characters other than `"` passed `check` and built
  on Linux, and failed on Windows with a bare OS error when the artifact was written; it is now
  `E_INVALID_PLACE_ID`. `"` never passed `check`, since a source string literal cannot carry one;
  it reaches the rule through a lockfile. The rule is the one
  `PlaceId`, `PortId` and `SiteName` share, so it covers port ids and site names too: a lockfile
  recording one of them is reported as such, and `PortId::new` / `SiteName::new` refuse them. From
  source only a `place id=` can carry these characters, since a port and a site are named by
  identifier tokens. `*`, `|`, `?`, `<`, `>`, `"` and U+0000 to U+001F are what a Windows file name
  cannot carry; U+007F to U+009F are refused because they print as nothing or move the cursor. A
  control character, or whitespace other than a plain space, is quoted as its escape in the
  message.

  Listed here rather than as a fix under `spec/compatibility` C.4: the spec gained these characters,
  rather than the build being brought in line with what it already said.
