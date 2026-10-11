use super::*;

fn translate(
    id: &str,
    properties: &IndexMap<String, String>,
) -> Result<StateTranslation, BedrockStateError> {
    translate_states(&BlockState {
        id: id.to_owned(),
        properties: properties.clone(),
    })
}

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
    let t = translate("minecraft:oak_planks", &IndexMap::new()).expect("bare");
    assert!(t.states.entries.is_empty());
    assert!(t.degraded.is_empty());
}

#[test]
fn stair_maps_facing_and_half_straight_is_lossless() {
    let t = translate(
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
        let t = translate(
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
    let t = translate(
        "minecraft:spruce_stairs",
        &stair_props("east", "top", "straight"),
    )
    .expect("stair");
    assert_eq!(t.states.entries.get("upside_down_bit"), Some(&Tag::Byte(1)));
}

#[test]
fn non_straight_shape_drops_as_the_property_and_the_value() {
    let t = translate(
        "minecraft:oak_stairs",
        &stair_props("south", "top", "outer_left"),
    )
    .expect("stair");
    // States carry only the mappable intent.
    assert_eq!(t.states.entries.get("weirdo_direction"), Some(&Tag::Int(2)));
    assert_eq!(t.states.entries.get("upside_down_bit"), Some(&Tag::Byte(1)));
    assert_eq!(t.states.entries.len(), 2);
    assert_eq!(
        t.degraded,
        [DroppedIntent::Shape {
            value: "outer_left".to_owned(),
        }],
    );
    // The warning is the note's sentence with the entry's Java state named.
    let detail = degradation_detail(&t.degraded[0]);
    assert!(
        detail.starts_with("shape=outer_left has no Bedrock state"),
        "got: {detail}"
    );
    let state = java_state(&BlockState {
        id: "minecraft:oak_stairs".to_owned(),
        properties: stair_props("south", "top", "outer_left"),
    });
    assert_eq!(
        degradation_message(&state, &t.degraded[0]),
        format!("stair `minecraft:oak_stairs[facing=south,half=top,shape=outer_left]` {detail}"),
    );

    // `straight` produces no note.
    let straight = translate(
        "minecraft:oak_stairs",
        &stair_props("south", "top", "straight"),
    )
    .expect("stair");
    assert!(straight.degraded.is_empty());
}

#[test]
fn the_family_test_agrees_with_the_one_that_decided_to_paint_it() {
    // `minecraft:mystairs` ends in `stairs` but not in `_stairs`, the id the
    // two plausible spellings of the stair rule disagree about; core does
    // not treat it as a stair. Pinned on behaviour rather than through the
    // predicate, so the test holds if the two stop sharing one.
    let err = translate(
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
    let err = translate("minecraft:oak_door", &props([("facing", "north")]))
        .expect_err("stateful non-stair");
    assert!(matches!(
        err,
        BedrockStateError::UnmappableBlock { ref id, .. } if id == "minecraft:oak_door"
    ));
    // The self-correction triple, `spec/versioning-editions` "Fail-loud
    // and minimum-version inference".
    let msg = err.to_string();
    assert!(msg.contains("minecraft:oak_door"), "wrong: {msg}");
    assert!(msg.contains("facing=north"), "wrong: {msg}");
    assert!(msg.contains("`minecraft:oak_planks`"), "valid: {msg}");
    assert!(msg.contains("`*_stairs`"), "valid: {msg}");
    assert!(msg.contains("--edition java"), "fix: {msg}");
}

#[test]
fn unknown_stair_facing_value_fails_loud() {
    let err = translate(
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
    let err = translate(
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
    assert!(
        msg.contains("correct the source blockstate")
            && msg.contains("Java has no such")
            && !msg.contains("or compile with"),
        "fix: {msg}",
    );
}

#[test]
fn unknown_stair_key_fails_loud() {
    let err = translate("minecraft:oak_stairs", &props([("waterlogged", "true")]))
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
