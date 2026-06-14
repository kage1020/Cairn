# 6. Blockstate Model

## 6.1 Derive by default + override-promotion (soft boundary)
- By default the compiler **derives** blockstate from position and neighbors (stair facing, door
  orientation, glass-pane/fence/wall connections, chest left/right, etc.).
- **Any blockstate that can be architectural intent is overridable, and the moment it is written it is
  promoted to "intent".**
- The strong reading "if it can be derived, don't let the AI write it" is **not** adopted. The correct
  rule is "derive by default; any blockstate that can be intent is overridable."

Representative cases that MUST be overridable (i.e. can be intent):
- `stairs facing` (as a chair/decoration the facing itself is intent), `stairs half=top` (upside-down
  = eaves), `stairs shape`
- `chest size=single` (auto-merging from adjacency is forbidden), `bed facing`, `door hinge/open`
- `log/pillar axis` (a horizontal beam), `trapdoor open/half`, `snow layers`, `candle count`,
  `glazed_terracotta` rotation
- `redstone_dust connect`, `repeater delay`, `observer/piston/dispenser facing`, `note/instrument`

Easy-to-miss cases that belong to derivation: `torch`↔`wall_torch`, `sign`↔`wall_sign` auto-substitution
by attachment face.

```
stair id=eave   kind=stairs mat_slot=roof side=front half=top facing=out shape=outer_left  # eaves
beam  id=lintel kind=pillar mat_slot=frame at=front.top axis=x                              # horizontal beam
chest id=store  at=inside.back size=single
note_block at=2,1,2 instrument=bit note=12
```

## 6.2 IR representation: separate intent_state / resolved_state
```yaml
member:
  id: eave
  type: block | block_entity | entity      # the distinction is the compiler's job but typed in the IR
  primitive: stairs
  intent_state:   { half: top, shape: outer_left }       # author intent. Edit diffs look only here
  resolved_state: { facing: north, waterlogged: false }  # derived result; paint-derived state goes here
```
- Named to avoid collision with Minecraft's term `blockstate`: `intent_state` / `resolved_state`.
- `bed` is treated as a **block member** (not an entity, to keep the IR types clean).
- For edit stability, do not mix resolved (derived/paint-origin) with intent (author).

## 6.3 waterlogged
- Default is **paint-derived**: when `fill fluid=water` overlaps a waterloggable block, the compiler
  sets waterlogged.
- A three-valued `waterlogged=auto|true|false` is allowed: to leave an air pocket inside a tank
  (explicit false), to distinguish source/flowing, and for version differences in the waterloggable
  table.
- Flowing water is made explicit with `flow=` / `level=`.

```
fill fluid=water kind=source from=1,1,1 to=5,3,5    # overlapping fences/stairs/signs auto-waterlogged
trapdoor id=shutter at=.. waterlogged=false          # air window in a tank
water id=stream from=.. flow=east level=4
```
