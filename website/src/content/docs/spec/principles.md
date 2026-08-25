---
title: "2. Design Principles"
---

## P1 — Separate intent from blockstate

The author declares meaning; the compiler derives blockstate. Two layers — meaning and blockstate —
do not scale to `def`, themes, and multi-building, so the model is a three-layer IR with named
members. See [Architecture](architecture).

## P2 — A phase-ordered canvas

Whole-program last-wins, the "paint model", produces order-dependent accidents, so it is dropped.
Commands are sorted into implicit phases and evaluated in a fixed order, and last-wins is restricted
to local overrides within one phase. See [Compilation Model](compilation).

## P3 — A small closed vocabulary, with an escape hatch

Keep the set of semantic primitives small. The smaller the vocabulary, the more stable LLM
generation is and the simpler the validator. Missing expressiveness escapes through `raw`
directives.

## P4 — Relative, semantic positioning

Position by wall selectors rather than by absolute coordinates. Blocks, block entities, and entities
share one selector grammar. See [Syntax](syntax).

## P5 — The lint loop is part of the spec

The compiler is both a translator and an architectural linter. The form and granularity of error
reporting are designed as first-class concerns, because precision is earned through a loop rather
than through one-shot generation. See [Lint](lint).
