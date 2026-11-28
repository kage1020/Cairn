- *(redstone)* Coupling reads the floor row and the row over it by block as well as by member. A
  block power moves, such as a trapdoor, note block or fence gate, counts as an actuator whatever
  member lays it, and one that gives power, such as a redstone block, counts as a sensor. A
  trapdoor beside a driven lamp is now `E_ROUTE_UNREACHABLE_COMPONENT`, and the top layer's dust
  stays out from under a floor of redstone blocks.
