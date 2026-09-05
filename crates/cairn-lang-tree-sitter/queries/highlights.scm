; keywords
[
  "theme" "struct" "def" "site" "slot" "logic" "assert"
] @keyword

; The member-level version floor's keyword, captured through its statement
; rather than added to the list above. The same literal also spells the
; second half of the `@requires` directive name, and there the sigil and
; the word are one keyword rather than two — listing the bare literal
; splits that span in half.
(requires_stmt "requires" @keyword)

; A member command's keyword is any identifier — `floor`, `walls`, and the
; rest are not reserved words, they are the names the compiler happens to
; know (see `member_keyword` in grammar.js). The node exists so this
; pattern can reach the one identifier that opens a statement without also
; matching the arguments that follow it.
(member_keyword) @keyword

; A theme selector row names the member kind it filters, in the same
; keyword position and reading as the same word.
(selector keyword: (identifier) @keyword)

"truth" @keyword.operator
"always" @keyword.operator
"eventually" @keyword.operator
"within" @keyword.operator
"or" @keyword.operator
"and" @keyword.operator
"not" @keyword.operator

; directives
(directive_name) @keyword.directive
; `@cairn` / `@requires` keep their value as one opaque run to end of line
; (the reference parser does not read it either), so it highlights as a
; single literal rather than being split into operator and number.
(directive_literal) @string.special

; operators
["->" "="] @operator

; punctuation
["[" "]" "(" ")" "{" "}"] @punctuation.bracket
["," ";" "." ":"] @punctuation.delimiter

; literals
(string) @string
(integer) @number
(bit) @number
(size_literal) @number.special
(boolean) @constant.builtin

; types / references
(material_ref) @type
(attribute key: (identifier) @variable.parameter)
; First segment stays as @variable (default, via the generic identifier
; fallback); every segment after the first `.` is member-like. The leading
; `.` anchors the first `(identifier)` to the start of the pattern; the
; trailing `(identifier)` is unanchored, so tree-sitter matches it once per
; remaining sibling — for `a.b.c.d` this yields @variable.member on b, c,
; and d (verified empirically with `tree-sitter query` against a synthetic
; 4-segment signal_ref).
(signal_ref . (identifier) (identifier) @variable.member)

; comments
(comment) @comment
