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
"latency" @keyword.operator
"or" @keyword.operator
"nor" @keyword.operator
"xor" @keyword.operator
"and" @keyword.operator
"nand" @keyword.operator
"not" @keyword.operator
"mux" @function.builtin
"latch" @function.builtin
"delay" @function.builtin
"pulse" @function.builtin
"edge_rising" @function.builtin
"edge_falling" @function.builtin
"counter" @function.builtin

; directives
(directive_name) @keyword.directive
(edition_guard "@" @keyword.directive "edition" @keyword.directive)
(edition_name) @constant.builtin
; One opaque run to end of line, which the reference parser does not read either.
(directive_literal) @string.special

; operators
["->" "=" "<="] @operator

; punctuation
["[" "]" "(" ")" "{" "}" "[B;" "[I;" "[L;"] @punctuation.bracket
["," ";" "." ":"] @punctuation.delimiter

; literals
(string) @string
(integer) @number
(decimal) @number.float
(nbt_number) @number
(nbt_string) @string
(bit) @number
(size_literal) @number.special
(coordinate) @number.special
(boolean) @constant.builtin

; types / references
(material_ref) @type
(block_id) @type
(attribute key: (identifier) @variable.parameter)
(nbt_key) @property
(operand_name) @variable.parameter
; Every segment after the first is a member; the first keeps the generic @variable.
(signal_ref . (identifier) (identifier) @variable.member)

; comments
(comment) @comment
