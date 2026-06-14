---
title: "9. Components, Editing, and Multi-building"
---

## 9.1 def (components)
`def` defines a slot-bearing Component, unified with `theme` and `site` by the same mechanism. This
keeps the reference system from fracturing across editing, theming, and multi-building connection.

- Parameterization (variable size, etc.) is allowed. Recursion is forbidden.
- A `def` may declare `requires version>=X`; the minimum version of a composite is the max of its
  parts ([Versioning and Editions](versioning-editions)).

```
def cottage class=house size=9x7:
  floor  id=floor mat_slot=floor
  walls  id=walls class=outer mat_slot=wall height=4
  door   id=door  class=entry side=front at=center
  roof   id=roof  kind=gable mat_slot=roof
```

## 9.2 Editing model
**Explicit IDs + auto stable addresses, combined.** Important members carry `id=`; unspecified members
get a **meaning-based stable address** auto-assigned by the compiler. Addresses derive from
parent / role / side / level / offset rather than generation order, so they stay stable under
appends.

Edits are patch DSL against a selector/address:

```
edit window[class=vent][level=floor2] set shape=arch
edit window@front[0]                  set mat_slot=accent_glass
edit door[id=entry]                   set side=front at=center
```

Editing at the level of a concept ("make only the second-floor windows arched") must be possible
without breaking the whole. Edit diffs look only at `intent_state` ([Blockstate Model](blockstate)),
so a change in derived results does not harm edit stability.

## 9.3 Multi-building (site)
Do not make the AI do absolute-coordinate arithmetic. Place via topological relational constraints;
resolving to absolute coordinates is the compiler's responsibility.

```
site village:
  place id=home1 use=cottage theme=medieval at=origin
  place id=home2 use=cottage theme=medieval east_of=home1 gap=4
  connect home1.door to home2.door path=@gravel
```

Each struct exposes ports (position / normal / width), and `connect` joins them. Villages and castles
that exceed the structure block's 48³ limit are expressed as the composition of multiple structs.
