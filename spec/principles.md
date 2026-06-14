# 2. Design Principles (P1–P5)

- **P1. Intent/meaning separation (three layers).** The AI declares meaning (intent); blockstate is
  derived by the compiler by default. Two layers (meaning and blockstate) do not scale to `def`,
  themes, and multi-building, so the model is extended to a three-layer IR with named members
  ([architecture.md](architecture.md)).

- **P2. Phase-ordered canvas.** Whole-program last-wins (the "paint model") produces order-dependent
  accidents and is dropped. Commands are auto-sorted into implicit phases and evaluated in a fixed
  order. Last-wins is restricted to local overrides within the same phase ([compilation.md](compilation.md)).

- **P3. Small closed vocabulary + escape hatch.** Keep the set of semantic primitives small; the
  smaller the vocabulary, the more stable LLM generation is and the easier the validator. Missing
  expressiveness escapes through raw directives.

- **P4. Relative, semantic positioning (selectors).** Position by wall selectors and the like rather
  than absolute coordinates. Blocks, block entities, and entities share one selector grammar
  ([syntax.md](syntax.md)).

- **P5. The lint self-correction loop is part of the spec.** The compiler is both a translator and an
  architectural linter. The form and granularity of error reporting are designed as first-class
  citizens ([lint.md](lint.md)). Precision is earned through a loop, not one-shot generation.
