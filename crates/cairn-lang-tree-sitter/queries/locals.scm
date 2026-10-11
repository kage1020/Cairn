; `attribute` has one shape in a selector, a command and a theme row, so no parent is pinned.
(attribute
  key: (identifier) @_key
  value: (identifier) @local.definition.member
  (#eq? @_key "id"))

; No `@local.reference`: most attribute values are enum literals, like `side=front`, not ids.
