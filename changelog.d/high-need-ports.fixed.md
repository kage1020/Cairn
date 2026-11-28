- *(redstone)* A `counter`'s `a` and a `latch`'s `reset` enter their cell by a one-tick repeater of
  their own, so the wire into each needs 1 and a sensor feeding one lays in whatever room the struct
  leaves it. Each adds a tick: a counter's port takes 4 ticks, and a latch's `reset` 6.
