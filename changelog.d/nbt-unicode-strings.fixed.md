- *(nbt,formats)* Every writer refused a string holding NUL or a character outside ASCII. A sign,
  a text display or an `nbt=` compound with such a character did not build, and neither did a
  structure stem or region name given one through the library API. The Java writers (`.nbt`,
  `.litematic` and `.schem`) now write each string in the Modified UTF-8 the game reads, NUL and
  characters past U+FFFF included. The Bedrock writer writes UTF-8. Each writer refuses only a
  string longer than the 65,535 bytes an NBT string holds. The refusal gives the bytes counted, the
  encoding they were counted in and how to fix it. For a sign line that a release before 1.21.5
  saves as a JSON string, it says that the quotes and escapes were counted.
