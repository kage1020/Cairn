- *(nbt,formats)* Every writer refused a string holding a character outside ASCII, so a sign,
  a text display or an `nbt=` compound written in any other script, a structure stem or a region
  name included, did not build. The Java writer now writes each string in the Modified UTF-8 the
  game reads, NUL and characters past U+FFFF included, and the Bedrock writer in UTF-8; both refuse
  only a string longer than the 65,535 bytes an NBT string holds, counted in that encoding, and the
  fix the refusal gives is to shorten the text.
