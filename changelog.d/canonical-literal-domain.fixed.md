- *(formats)* On Bedrock, a canonical token's state literal is checked against its own Java block's
  states, as on Java: `@torch[facing=east]` is `E_STATE_DOMAIN` at the token rather than a wall
  torch, and `@cauldron[level=2]` is `E_STATE_DOMAIN` at the token rather than `E_STATE_UNMAPPED`
  at the member.
