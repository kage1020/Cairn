- *(core)* `and`, `or` and `not` are primitives of a built-in logic library,
  `logic_library::LogicPrimitive`, each with a name and a notation: prefix, or infix with a rank.
  `ast::Expr::And`, `Expr::Or` and `Expr::Not` are gone, replaced by
  `Expr::Infix { primitive, lhs, rhs }` and `Expr::Prefix { primitive, operand }`, so both shapes
  `cairn parse --format json` prints for an expression change:
  - `{"kind": "And", "value": [<lhs>, <rhs>]}` becomes
    `{"kind": "Infix", "value": {"primitive": "and", "lhs": <lhs>, "rhs": <rhs>}}`, and `Or` the
    same way.
  - `{"kind": "Not", "value": <operand>}` becomes
    `{"kind": "Prefix", "value": {"primitive": "not", "operand": <operand>}}`.

  Every `logic` line that parsed before groups the same way, and one that was refused is still
  refused at the same position.
