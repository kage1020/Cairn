- *(core)* The AST keeps the bare values of the lines that are not members. `Item::Theme`,
  `Item::Def`, `Item::Site`, `Item::Struct`, `ThemeRule::Slot`, `ThemeRule::Selector`,
  `Statement::AssertTruth`, `Statement::AssertAlways` and `Statement::AssertLatency` gain the field
  `positional`, a `Vec<Value>`, and `Statement::Logic` the field `binds`, a `bool`. `Item::Site`,
  `Item::Struct`, both `ThemeRule` variants, `Statement::Logic` and the three assert variants are
  now `#[non_exhaustive]`, so outside the crate a literal of one no longer compiles, nor does a
  pattern on one written without `..`.
