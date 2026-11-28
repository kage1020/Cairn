- *(redstone)* `E_ATTENUATION_LIMIT` for an input no buffer repeater lifts names the input and the
  signal it reads, the strength it reads and needs, and the blocks of dust the signal runs over,
  and says they are more than 15 only where they are: a four-block wire into a `counter` port that
  needs 14 was reported as dust running past 15 blocks. Its fix offers moving what drives the
  signal nearer, or lining it up with the input, rather than widening the struct.
