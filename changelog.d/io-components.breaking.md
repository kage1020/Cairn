- *(core)* `lever`, `button`, `daylight`, `observer`, `lamp`, `piston` and `dispenser` are member
  keywords, so a line headed by one is no longer `E_UNKNOWN_KEYWORD`, and `lit_by=`, `powered_by=`
  and `fired_by=` bind the component that reads them instead of being refused everywhere.
- *(core)* `MemberRole` gains `Lever`, `Button`, `Daylight`, `Observer`, `Lamp`, `Piston` and
  `Dispenser`, so an exhaustive match on it no longer compiles.
- *(core)* `E_INCOMPATIBLE_MATERIAL`'s `required` takes the new value `button`.
- *(formats)* Both built-in packs' hashes move with the eight new tier-1 tokens.
