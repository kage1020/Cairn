; An `id=<ident>` attribute — inside a bracketed selector (e.g.
; `door[id=front]`), among a member command's args (e.g. `floor id=floor`,
; `level id=floor1 y=0`), or in a theme selector's bindings — defines a
; local reference target. `attribute` has the identical key/value field
; shape in every one of these positions (see grammar.js: the selector's
; `filter_list`, a command's `command_arg_list`, and a theme row's
; `attribute_list` all hold the same node), so matching on `attribute`
; directly, without pinning the parent, covers all of them. Verified
; against examples/: every `id=` in the corpus appears in one of these
; positions; no struct/def/site header uses `id=`, so no separate pattern
; is added for those.
(attribute
  key: (identifier) @_key
  value: (identifier) @local.definition.member
  (#eq? @_key "id"))

; Deliberately no `@local.reference` pattern here. A pattern that captured
; every non-"id" identifier attribute value (the previous approach) was
; over-broad: it tagged coincidental text matches like `side=front` as a
; reference to an unrelated `id=front` in the same statement, since most
; attribute values are enum literals (side=, class=, kind=, mat_slot=, ...),
; not id lookups. The grammar has no concrete list of attribute keys that
; reliably hold an id reference (e.g. `opened_by=` takes a signal_ref, not
; an id). Ship only `@local.definition.member` until specific
; reference-bearing keys are identified, then add a narrow pattern keyed on
; those exact attribute names.
