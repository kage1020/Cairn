; keywords
[
  "theme" "struct" "def" "site" "slot" "logic" "assert"
] @keyword

(member_keyword) @keyword
(nested_scope keyword: (identifier) @keyword)

"truth" @keyword.operator
"always" @keyword.operator
"eventually" @keyword.operator
"within" @keyword.operator
"or" @keyword.operator
"and" @keyword.operator
"not" @keyword.operator

; directives
(directive_name) @keyword.directive

; operators
["->" "=" ">=" "<=" ">" "<"] @operator

; punctuation
["[" "]" "(" ")" "{" "}"] @punctuation.bracket
["," ";" "." ":"] @punctuation.delimiter

; literals
(string) @string
(integer) @number
(bit_pattern) @number
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
