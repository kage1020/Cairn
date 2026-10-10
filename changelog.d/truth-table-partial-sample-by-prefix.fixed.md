- *(core)* A partial truth table whose first unassigned combination sat behind more combinations
  than the search for a sample would step through lost its `W_TRUTH_TABLE_PARTIAL` although the
  count was exact. A row fixing the lowest of fifteen inputs beside a row fixing the lowest and
  the highest is one: its first gap comes after 16 385 assigned combinations, past that search's
  cap of 10 000 steps. The sample is now found by descending through prefixes, so the finding no
  longer depends on how many combinations sit in front of the first gap, and it names the lowest
  missing combinations, up to four. It is still withheld when the rows stand for `2^64`
  combinations or more between them and leave some out, which takes 65 inputs or more, because
  the payload carries the covered count as a 64-bit integer.
