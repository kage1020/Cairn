- *(core)* Lowering records each member's `resolved_state`: every block state the member's cells
  hold once its body is painted, with those cells as runs along `x`. A cell belongs to the member
  that painted it last. Its block ids are those the lowering's target resolved, and its state names
  the canonical vocabulary's. `BlockArrayIr::members` holds the record for each struct and place
  body, keyed like `structures`, in the order of the first cell each member owns, so reordering
  lines that share no cell leaves it as it was. `cairn lower --format json` prints it under
  `members`, so the states a roof, a stair or an opening resolved to can be read, and compared
  between two builds, member by member.
