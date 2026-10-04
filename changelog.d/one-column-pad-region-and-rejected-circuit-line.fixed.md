- *(core,redstone)* An identity wire in a region one column wide passed placement with its sensor
  and actuator pads on one voxel, and routing then asked for more depth, which does not help.
  Placement now refuses it with an `E_ROUTE_CONGESTION` that asks for two columns.

- *(core,redstone)* A scope whose `circuit` line was unusable got an `E_NO_CIRCUIT_REGION` on its
  first `logic` line that listed every possible cause. It now stands on the `circuit` line and
  names the one that applies, with a fix for it.
