- *(core)* A `pressure_plate` whose `mat_slot=` binds a block that is no pressure plate, such as
  `@cobblestone`, is `E_INCOMPATIBLE_MATERIAL`, and the plate is built from
  `@pressure_plate.default`, as a `button` outside its family is. It used to paint the bound block
  where the plate should stand, still wired into the circuit, with no finding. The payload's
  `required` takes the new value `pressure_plate`.
