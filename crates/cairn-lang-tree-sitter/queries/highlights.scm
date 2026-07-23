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
(signal_ref (identifier) . (identifier) @variable.member)

; comments
(comment) @comment
