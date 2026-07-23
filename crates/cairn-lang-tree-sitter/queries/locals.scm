; An `id=<ident>` attribute (whether inside a bracketed selector, e.g.
; `door[id=front]`, or a plain command arg, e.g. `floor id=floor`) defines a
; local reference other attributes can point back to.
(attribute
  key: (identifier) @_key
  value: (identifier) @local.definition.member
  (#eq? @_key "id"))

; Any other bare-identifier attribute value is a reference to that id.
(attribute
  key: (identifier) @_key
  value: (identifier) @local.reference
  (#not-eq? @_key "id"))
