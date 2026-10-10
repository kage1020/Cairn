- *(core)* `intent::Member::mat_slot` is retyped from `Option<String>` to `Option<Label>`. The new
  `intent::Label` carries the slot name (`name`) and where its value was written (`span`), so a pass
  can report on a hoisted `mat_slot=` at its value. It still serialises as the bare name. Code that
  read the field as a string reads `.name`, and code that builds a `Member` with a struct literal
  wraps the name in a `Label`.
- *(core)* `MemberRole::unread_arguments` is retyped from `&'static [&'static str]` to
  `&'static [UnreadArgument]`. Each entry carries its `key` and why nothing on the role reads it, as
  the new `intent::Unread`: `Unreached`, a key the specification defines and no pass reads yet, or
  `Inapplicable`, a universal key on a role that puts down nothing it would be read for. The new
  `MemberRole::unread_argument` answers for one key.
