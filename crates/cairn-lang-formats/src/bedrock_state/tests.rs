use super::*;

fn props<const N: usize>(pairs: [(&str, &str); N]) -> IndexMap<String, String> {
    pairs
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

/// Stair properties in the `facing` / `half` / `shape` insertion order the
/// lowering uses, so `translate_states` sees the same key stream as at
/// runtime.
fn stair_props(facing: &str, half: &str, shape: &str) -> IndexMap<String, String> {
    props([("facing", facing), ("half", half), ("shape", shape)])
}

#[test]
fn bare_block_has_empty_states_and_no_degradation() {
    let t = translate_states("minecraft:oak_planks", &IndexMap::new()).expect("bare");
    assert!(t.states.entries.is_empty());
    assert!(t.degraded.is_empty());
}

#[test]
fn stair_maps_facing_and_half_straight_is_lossless() {
    let t = translate_states(
        "minecraft:dark_oak_stairs",
        &stair_props("north", "bottom", "straight"),
    )
    .expect("stair");
    assert_eq!(t.states.entries.get("weirdo_direction"), Some(&Tag::Int(3)));
    assert_eq!(t.states.entries.get("upside_down_bit"), Some(&Tag::Byte(0)));
    // `shape=straight` and no other keys leak in.
    assert_eq!(t.states.entries.len(), 2);
    assert!(t.degraded.is_empty());
}

#[test]
fn all_four_facings_map_to_distinct_weirdo_directions() {
    let expected = [("east", 0), ("west", 1), ("south", 2), ("north", 3)];
    for (facing, weirdo) in expected {
        let t = translate_states(
            "minecraft:oak_stairs",
            &stair_props(facing, "bottom", "straight"),
        )
        .expect("stair");
        assert_eq!(
            t.states.entries.get("weirdo_direction"),
            Some(&Tag::Int(weirdo)),
            "facing={facing}"
        );
    }
}

#[test]
fn half_top_sets_upside_down_bit() {
    let t = translate_states(
        "minecraft:spruce_stairs",
        &stair_props("east", "top", "straight"),
    )
    .expect("stair");
    assert_eq!(t.states.entries.get("upside_down_bit"), Some(&Tag::Byte(1)));
}

#[test]
fn non_straight_shape_drops_as_the_property_and_the_value() {
    let t = translate_states(
        "minecraft:oak_stairs",
        &stair_props("south", "top", "outer_left"),
    )
    .expect("stair");
    // States carry only the mappable intent.
    assert_eq!(t.states.entries.get("weirdo_direction"), Some(&Tag::Int(2)));
    assert_eq!(t.states.entries.get("upside_down_bit"), Some(&Tag::Byte(1)));
    assert_eq!(t.states.entries.len(), 2);
    // One dropped intent, as the pieces: a consumer of
    // `degraded_entries` reads which property was lost without
    // parsing it back out of a sentence.
    assert_eq!(
        t.degraded,
        [DroppedIntent::Shape {
            value: "outer_left".to_owned(),
        }],
    );
    // The sentence is written once, from those pieces, and the
    // warning's form is the note's with the entry's Java state named.
    let detail = degradation_detail(&t.degraded[0]);
    assert!(
        detail.starts_with("shape=outer_left has no Bedrock state"),
        "got: {detail}"
    );
    let state = java_state(
        "minecraft:oak_stairs",
        &stair_props("south", "top", "outer_left"),
    );
    assert_eq!(
        degradation_message(&state, &t.degraded[0]),
        format!("stair `minecraft:oak_stairs[facing=south,half=top,shape=outer_left]` {detail}"),
    );

    // `straight` produces no note.
    let straight = translate_states(
        "minecraft:oak_stairs",
        &stair_props("south", "top", "straight"),
    )
    .expect("stair");
    assert!(straight.degraded.is_empty());
}

#[test]
fn the_family_test_agrees_with_the_one_that_decided_to_paint_it() {
    // `minecraft:mystairs` is the id the two plausible spellings of the
    // stair rule disagree about: its path ends in `stairs` but not in
    // `_stairs`. `cairn-lang-core` refuses to attach stair states to it,
    // and this module has to refuse to translate them — a second copy of
    // the rule that answered "stair" here would translate a blockstate
    // core never intended to exist, with nothing between it and the
    // written file.
    //
    // Pinned on behaviour rather than by calling the predicate, so the
    // test still means something if the two ever stop sharing one.
    let err = translate_states(
        "minecraft:mystairs",
        &stair_props("south", "top", "straight"),
    )
    .expect_err("not a stair");
    assert!(matches!(
        err,
        BedrockStateError::UnmappableBlock { ref id, .. } if id == "minecraft:mystairs"
    ));
}

#[test]
fn non_stair_with_properties_fails_loud() {
    let err = translate_states("minecraft:oak_door", &props([("facing", "north")]))
        .expect_err("stateful non-stair");
    assert!(matches!(
        err,
        BedrockStateError::UnmappableBlock { ref id, .. } if id == "minecraft:oak_door"
    ));
    // Self-correction triple per `spec/versioning-editions` "Fail-loud
    // and minimum-version inference": what is wrong / what is valid /
    // suggested fix. Each fragment is pinned so a message reword that
    // breaks the lint loop's expectation fails here first.
    let msg = err.to_string();
    assert!(msg.contains("minecraft:oak_door"), "wrong: {msg}");
    assert!(msg.contains("facing=north"), "wrong: {msg}");
    assert!(msg.contains("`minecraft:oak_planks`"), "valid: {msg}");
    assert!(msg.contains("`*_stairs`"), "valid: {msg}");
    assert!(msg.contains("--edition java"), "fix: {msg}");
}

#[test]
fn unknown_stair_facing_value_fails_loud() {
    let err = translate_states(
        "minecraft:oak_stairs",
        &stair_props("up", "bottom", "straight"),
    )
    .expect_err("bad facing");
    assert!(matches!(
        err,
        BedrockStateError::UnknownStairState { key: "facing", ref value, .. } if value == "up"
    ));
    let msg = err.to_string();
    assert!(msg.contains("facing=up"), "wrong: {msg}");
    // Valid values (FACING_VALID) surface in the message so the lint
    // loop can steer back to the closed domain instead of guessing.
    assert!(msg.contains("east, west, south, north"), "valid: {msg}");
    // The value is no more valid on Java, where it would be written
    // unchanged, so the fix does not send the author there.
    assert!(
        msg.contains("correct the source blockstate")
            && msg.contains("Java has no such")
            && !msg.contains("or compile with"),
        "fix: {msg}",
    );
}

#[test]
fn unknown_stair_half_value_fails_loud() {
    // Mirrors the facing test to guard against a future
    // `upside_down_bit()` refactor that silently maps unknown values to
    // `0` — a silent drop that `spec/versioning-editions` "Fail-loud and
    // minimum-version inference" forbids.
    let err = translate_states(
        "minecraft:oak_stairs",
        &stair_props("north", "middle", "straight"),
    )
    .expect_err("bad half");
    assert!(matches!(
        err,
        BedrockStateError::UnknownStairState { key: "half", ref value, .. } if value == "middle"
    ));
    let msg = err.to_string();
    assert!(msg.contains("half=middle"), "wrong: {msg}");
    assert!(msg.contains("top, bottom"), "valid: {msg}");
    // The value is no more valid on Java, where it would be written
    // unchanged, so the fix does not send the author there.
    assert!(
        msg.contains("correct the source blockstate")
            && msg.contains("Java has no such")
            && !msg.contains("or compile with"),
        "fix: {msg}",
    );
}

#[test]
fn unknown_stair_key_fails_loud() {
    let err = translate_states("minecraft:oak_stairs", &props([("waterlogged", "true")]))
        .expect_err("unknown key");
    assert!(matches!(
        err,
        BedrockStateError::UnknownStairKey { ref key, .. } if key == "waterlogged"
    ));
    let msg = err.to_string();
    assert!(msg.contains("waterlogged"), "wrong: {msg}");
    assert!(msg.contains("Handled: facing, half, shape"), "valid: {msg}");
    assert!(msg.contains("--edition java"), "fix: {msg}");
}
