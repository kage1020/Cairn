- *(redstone)* Delay insertion no longer steps below strength 0 when a Java `and` reads a net only
  an unplaced `pressure_plate` feeds, which panicked `check`, `compile`, `info` and the language
  server's `cairn.info` in debug builds and wrapped to strengths past 15 in release ones. A gate
  whose rear input reads nothing now drives nothing to measure, and nothing is buffered for it.
