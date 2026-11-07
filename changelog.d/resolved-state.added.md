- *(core)* Lowering records each member's `resolved_state`: every block state the member's cells
  hold once its body is painted, with those cells, in the canonical vocabulary. A cell belongs to
  the member that painted it last. `BlockArrayIr::members` holds the record for each struct and
  place body, keyed like `structures`, and `cairn lower --format json` prints it under `members`, so
  the states a roof, a stair or an opening resolved to can be read, and compared between two
  builds, member by member.
