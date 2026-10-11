; keywords
[
  "theme" "struct" "def" "site" "slot" "logic" "assert"
] @keyword

; Captured through its statement: the bare literal would split the `@requires` directive name.
(requires_stmt "requires" @keyword)

; Member keywords are identifiers, not reserved words.
(member_keyword) @keyword

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
; One opaque run to end of line, which the reference parser does not read either.
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
; Every segment after the first is a member; the first keeps the generic @variable.
(signal_ref . (identifier) (identifier) @variable.member)

; comments
(comment) @comment
