- *(redstone)* An identity wire in a region one column wide (`size=1xN`) passed `--stage placement`
  at exit 0, with a Placement IR whose output pad stood on `(0,0,0)`, the same voxel as its first
  input pad. Placement now refuses it with an `E_ROUTE_CONGESTION` that asks for a second column.
  Its `Fix:` line asks for that alone, where the refusal `--stage route` gave before went on to the
  depth rule and to splitting into multiple `circuit` blocks.

- *(core,redstone)* A scope whose `circuit` line reserved nothing got an `E_NO_CIRCUIT_REGION` on
  its first `logic` line (or its first actuator binding, with no `logic` line) that listed every
  possible cause. It now stands on that `circuit` line, names the one cause, and gives a fix for
  it; with several such lines, the first. A `circuit` line under a `level`, which reserves nothing,
  is named as such on the line. A scope with no `circuit` line keeps the old anchor, and its
  message now asks only for the line.
