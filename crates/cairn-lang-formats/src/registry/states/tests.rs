use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockState;
use indexmap::IndexMap;
use serde_json::{Value, json};

use super::*;
use crate::registry::blocks::{BlocksCatalog, BlocksIndex};

/// Two Bedrock versions whose blocks the fixture's families write.
fn blocks() -> BlocksIndex {
    let catalog: BlocksCatalog = serde_json::from_value(json!({
        "schema_version": 2,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": {
            "oak_stairs": { "upside_down_bit": [false, true], "weirdo_direction": [0, 1, 2, 3] },
            "oak_slab": { "minecraft:vertical_half": ["bottom", "top"] },
            "oak_double_slab": { "minecraft:vertical_half": ["bottom", "top"] },
            "oak_fence": {},
            "stonebrick": { "stone_brick_type": ["default", "mossy"] },
            "log": { "old_log_type": ["oak", "birch"], "pillar_axis": ["x", "y", "z"] },
            "standing_sign": { "ground_sign_direction": [0, 1, 2, 3] },
            "cherry_wood": { "pillar_axis": ["y"] },
            "jigsaw": { "facing_direction": [0, 1] },
            "frame": { "facing_direction": [0, 1] },
            "creeper_head": { "facing_direction": [0, 1] },
            "kelp": { "kelp_age": [0, 1] },
            "wall_banner": { "facing_direction": [2, 3] },
            "bed": { "head_piece_bit": [false, true] },
            "birch_slab": { "minecraft:vertical_half": ["bottom", "top"] },
            "birch_double_slab": { "minecraft:vertical_half": ["bottom", "top"] }
        } },
        "diffs": [{
            "mc_version": "1.1",
            "inherits": "1.0",
            "changed": { "cherry_wood": { "pillar_axis": ["x", "y", "z"] } }
        }]
    }))
    .expect("the fixture's blocks parse");
    BlocksIndex::from_catalog(catalog, Edition::Bedrock).expect("the fixture's blocks fold")
}

fn both() -> Value {
    json!(["1.0", "1.1"])
}

fn waterlogged() -> Value {
    json!(["true", "false"])
}

/// A state map exercising each way an entry resolves.
fn states() -> Value {
    let axis = json!({ "reads": ["axis"], "map": { "x": "x", "y": "y", "z": "z" } });
    let sign = |java: &str| {
        json!({
            "mc_versions": both(), "ids": [format!("standing_sign={java}")],
            "java": { "rotation": ["0", "1", "2", "3"] },
            "defaults": { "rotation": "0" },
            "states": { "ground_sign_direction":
                { "reads": ["rotation"], "map": { "0": 0, "1": 1, "2": 2, "3": 3 } } }
        })
    };
    let log = |java: &str, kind: &str| {
        json!({
            "mc_versions": both(), "ids": [format!("log={java}")],
            "java": { "axis": ["x", "y", "z"] },
            "defaults": { "axis": "y" },
            "states": { "old_log_type": { "value": kind }, "pillar_axis": axis }
        })
    };
    let mut catalog = json!({
        "schema_version": 1,
        "namespace": "minecraft",
        "refused": { "1.0": ["jigsaw", "skull=creeper_head"] },
        "in_water": ["kelp"],
        "families": [
            {
                "mc_versions": both(), "ids": ["oak_stairs"],
                "java": {
                    "facing": ["north", "south", "west", "east"],
                    "half": ["top", "bottom"],
                    "shape": ["straight", "outer_left"],
                    "waterlogged": waterlogged()
                },
                "defaults": {
                    "facing": "north", "half": "bottom", "shape": "straight", "waterlogged": "false"
                },
                "states": {
                    "upside_down_bit": { "reads": ["half"], "map": { "top": true, "bottom": false } },
                    "weirdo_direction": { "reads": ["facing"],
                        "map": { "north": 3, "south": 2, "west": 1, "east": 0 } }
                },
                "dropped": ["shape"]
            },
            {
                "mc_versions": both(), "ids": ["oak_slab"],
                "java": { "type": ["top", "bottom"], "waterlogged": waterlogged() },
                "defaults": { "type": "bottom", "waterlogged": "false" },
                "states": { "minecraft:vertical_half":
                    { "reads": ["type"], "map": { "top": "top", "bottom": "bottom" } } }
            },
            {
                "mc_versions": both(), "ids": ["oak_double_slab=oak_slab"],
                "java": { "type": ["double"], "waterlogged": waterlogged() },
                "defaults": { "type": "bottom", "waterlogged": "false" },
                "states": { "minecraft:vertical_half": { "value": "bottom" } }
            },
            {
                "mc_versions": both(), "ids": ["oak_fence"],
                "java": { "east": ["true", "false"], "north": ["true", "false"] },
                "defaults": { "east": "false", "north": "false" },
                "states": {},
                "dropped": ["east", "north"]
            },
            {
                "mc_versions": both(), "ids": ["stonebrick=stone_bricks"],
                "java": {}, "defaults": {},
                "states": { "stone_brick_type": { "value": "default" } }
            },
            {
                "mc_versions": both(), "ids": ["stonebrick=mossy_stone_bricks"],
                "java": {}, "defaults": {},
                "states": { "stone_brick_type": { "value": "mossy" } }
            },
            log("oak_log", "oak"),
            log("birch_log", "birch"),
            sign("oak_sign"),
            sign("spruce_sign"),
            {
                "mc_versions": ["1.0"], "ids": ["cherry_wood"],
                "java": { "axis": ["y"] }, "defaults": { "axis": "y" },
                "states": { "pillar_axis": { "value": "y" } }
            },
            {
                "mc_versions": ["1.1"], "ids": ["cherry_wood"],
                "java": { "axis": ["x", "y", "z"], "waterlogged": waterlogged() },
                "defaults": { "axis": "y", "waterlogged": "false" },
                "states": { "pillar_axis": axis }
            }
        ]
    });
    catalog["families"]
        .as_array_mut()
        .expect("the fixture lists families")
        .extend(edge_families());
    catalog
}

/// Families only an edge of loading or resolution needs, after the ones tests name by place.
fn edge_families() -> Vec<Value> {
    vec![
        json!({
            "mc_versions": both(), "ids": ["kelp"],
            "java": { "age": ["0", "1"] }, "defaults": { "age": "0" },
            "states": { "kelp_age": { "reads": ["age"], "map": { "0": 0, "1": 1 } } }
        }),
        json!({
            "mc_versions": both(), "ids": ["wall_banner=white_wall_banner"],
            "java": { "facing": ["south", "west"] }, "defaults": { "facing": "north" },
            "states": { "facing_direction":
                { "reads": ["facing"], "map": { "south": 3, "west": 2 } } }
        }),
        json!({
            "mc_versions": both(), "ids": ["bed=white_bed"],
            "java": { "occupied": ["false"], "part": ["head"] },
            "defaults": { "occupied": "false", "part": "foot" },
            "states": { "head_piece_bit": { "value": true } }
        }),
        json!({
            "mc_versions": both(), "ids": ["bed=red_bed"],
            "java": { "occupied": ["true"], "part": ["foot"] },
            "defaults": { "occupied": "false", "part": "foot" },
            "states": { "head_piece_bit": { "value": false } }
        }),
        json!({
            "mc_versions": both(), "ids": ["birch_slab"],
            "java": { "type": ["top"], "waterlogged": ["false"] },
            "defaults": { "type": "bottom", "waterlogged": "false" },
            "states": { "minecraft:vertical_half": { "value": "top" } }
        }),
        json!({
            "mc_versions": both(), "ids": ["birch_double_slab=birch_slab"],
            "java": { "type": ["double"], "waterlogged": ["true"] },
            "defaults": { "type": "bottom", "waterlogged": "false" },
            "states": { "minecraft:vertical_half": { "value": "bottom" } }
        }),
    ]
}

fn sensitivity() -> Value {
    json!({
        "schema_version": 1,
        "namespace": "minecraft",
        "dropped_states": {
            "east": [{ "kind": "derived", "reason": "connections follow the neighbours" }],
            "north": [{ "kind": "derived", "reason": "connections follow the neighbours" }],
            "shape": [{ "kind": "missing_state", "reason": "stairs render straight" }]
        }
    })
}

fn load(states: Value, sensitivity: Value) -> Result<StateMapIndex, StatesError> {
    StateMapIndex::from_catalogs(
        serde_json::from_value(states).expect("the states fixture parses"),
        serde_json::from_value(sensitivity).expect("the sensitivity fixture parses"),
        &blocks(),
    )
}

/// The object holding the entry at `pointer`, with the entry's key.
fn parent<'a>(
    catalog: &'a mut Value,
    pointer: &'a str,
) -> (&'a mut serde_json::Map<String, Value>, &'a str) {
    let (parent, key) = pointer.rsplit_once('/').expect("a pointer into an object");
    let object = catalog
        .pointer_mut(parent)
        .and_then(Value::as_object_mut)
        .unwrap_or_else(|| panic!("the fixture has an object at `{parent}`"));
    (object, key)
}

/// `catalog` with `value` set at `pointer`.
fn with(mut catalog: Value, pointer: &str, value: Value) -> Value {
    let (object, key) = parent(&mut catalog, pointer);
    object.insert(key.to_owned(), value);
    catalog
}

/// `catalog` without the entry at `pointer`.
fn without(mut catalog: Value, pointer: &str) -> Value {
    let (object, key) = parent(&mut catalog, pointer);
    object
        .remove(key)
        .unwrap_or_else(|| panic!("the fixture has `{pointer}`"));
    catalog
}

fn resolve(version: &str, id: &str, pairs: &[(&str, &str)]) -> Result<BedrockForm, StateRefusal> {
    resolve_in(
        &load(states(), sensitivity()).expect("the fixture loads"),
        version,
        id,
        pairs,
    )
}

fn resolve_in(
    index: &StateMapIndex,
    version: &str,
    id: &str,
    pairs: &[(&str, &str)],
) -> Result<BedrockForm, StateRefusal> {
    let properties: IndexMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    index
        .at(version)
        .expect("the fixture maps the version")
        .resolve(&BlockState {
            id: id.to_owned(),
            properties,
        })
}

fn form(id: &str, states: &[(&str, StateValue)]) -> BedrockForm {
    BedrockForm {
        id: id.to_owned(),
        states: states
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
        waterlogged: false,
        dropped: Vec::new(),
        rederived: Vec::new(),
    }
}

fn s(value: &str) -> StateValue {
    StateValue::Str(value.to_owned())
}

fn names(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}

fn unmapped(reason: &str) -> StateRefusal {
    StateRefusal::Unmapped {
        reason: reason.to_owned(),
    }
}

/// The refusal of `id`, whose states the version's palette does not declare.
fn palette_refused(id: &str) -> StateRefusal {
    unmapped(&format!(
        "its palette does not declare the states GeyserMC maps `{id}` to, so the pack carries no \
         form for them"
    ))
}

#[test]
fn the_fixture_maps_each_version_it_names_in_order() {
    let index = load(states(), sensitivity()).expect("the fixture loads");
    assert_eq!(index.versions().collect::<Vec<_>>(), ["1.0", "1.1"]);
    assert!(index.at("1.2").is_none());
}

#[test]
fn a_newer_schema_of_either_component_is_refused() {
    assert_eq!(
        load(with(states(), "/schema_version", json!(2)), sensitivity()).unwrap_err(),
        StatesError::UnsupportedSchemaVersion {
            component: "states",
            got: 2,
            supported: 1,
        },
    );
    assert_eq!(
        load(states(), with(sensitivity(), "/schema_version", json!(3))).unwrap_err(),
        StatesError::UnsupportedSchemaVersion {
            component: "sensitivity",
            got: 3,
            supported: 1,
        },
    );
}

#[test]
fn a_version_the_block_tables_lack_is_refused() {
    let unknown = StatesError::UnknownVersion {
        mc_version: "9.9".to_owned(),
    };
    let row = with(states(), "/families/0/mc_versions", json!(["1.0", "9.9"]));
    assert_eq!(load(row, sensitivity()).unwrap_err(), unknown);
    let refused = with(states(), "/refused", json!({ "9.9": ["frame"] }));
    assert_eq!(load(refused, sensitivity()).unwrap_err(), unknown);
}

#[test]
fn a_pair_both_mapped_and_refused_at_one_version_is_refused() {
    let catalog = with(states(), "/refused", json!({ "1.0": ["oak_fence"] }));
    assert_eq!(
        load(catalog, sensitivity()).unwrap_err(),
        StatesError::RefusedAndMapped {
            mc_version: "1.0".to_owned(),
            pair: "oak_fence".to_owned(),
        },
    );
}

#[test]
fn a_block_in_water_no_family_maps_is_refused() {
    let catalog = with(states(), "/in_water", json!(["seagrass"]));
    assert_eq!(
        load(catalog, sensitivity()).unwrap_err(),
        StatesError::InWaterUnmapped {
            java: "minecraft:seagrass".to_owned(),
        },
    );
}

#[test]
fn a_rule_with_a_key_of_neither_shape_does_not_parse() {
    for rule in [
        json!({ "reads": ["half"], "map": { "top": true, "bottom": false }, "typo": 5 }),
        json!({ "reads": ["half"], "map": { "top": true, "bottom": false }, "value": true }),
    ] {
        let catalog = with(states(), "/families/0/states/upside_down_bit", rule);
        assert!(
            serde_json::from_value::<StatesCatalog>(catalog).is_err(),
            "a rule with an extra key parsed",
        );
    }
}

#[test]
fn a_pair_onto_an_undeclared_block_is_refused() {
    let catalog = with(
        states(),
        "/families/0/ids",
        json!(["acacia_stairs=oak_stairs"]),
    );
    assert_eq!(
        load(catalog, sensitivity()).unwrap_err(),
        StatesError::UnknownBlock {
            mc_version: "1.0".to_owned(),
            id: "minecraft:acacia_stairs".to_owned(),
        },
    );
}

#[test]
fn a_state_or_value_the_palette_lacks_is_refused() {
    let value = with(
        states(),
        "/families/0/states/weirdo_direction/map/north",
        json!(4),
    );
    assert_eq!(
        load(value, sensitivity()).unwrap_err(),
        StatesError::OutsidePalette {
            mc_version: "1.0".to_owned(),
            id: "minecraft:oak_stairs".to_owned(),
            state: "weirdo_direction".to_owned(),
            value: "4".to_owned(),
        },
    );
    let state = with(
        states(),
        "/families/3/states",
        json!({ "connected_bit": { "value": true } }),
    );
    assert_eq!(
        load(state, sensitivity()).unwrap_err(),
        StatesError::OutsidePalette {
            mc_version: "1.0".to_owned(),
            id: "minecraft:oak_fence".to_owned(),
            state: "connected_bit".to_owned(),
            value: "true".to_owned(),
        },
    );
    let unwritten = with(
        without(states(), "/families/0/states/upside_down_bit"),
        "/families/0/dropped",
        json!(["shape", "half"]),
    );
    assert_eq!(
        load(unwritten, sensitivity()).unwrap_err(),
        StatesError::StateUnwritten {
            mc_version: "1.0".to_owned(),
            id: "minecraft:oak_stairs".to_owned(),
            state: "upside_down_bit".to_owned(),
        },
    );
}

#[test]
fn a_pair_two_rows_map_at_one_version_is_refused() {
    let mut catalog = states();
    let copy = catalog["families"][0].clone();
    catalog["families"]
        .as_array_mut()
        .expect("families is a list")
        .push(copy);
    assert_eq!(
        load(catalog, sensitivity()).unwrap_err(),
        StatesError::DuplicatePair {
            mc_version: "1.0".to_owned(),
            pair: "oak_stairs".to_owned(),
        },
    );
}

#[test]
fn a_family_at_odds_with_its_properties_is_malformed() {
    let stairs = "/families/0";
    let cases = [
        (
            with(states(), &format!("{stairs}/java/shape"), json!([])),
            "a property holds no value",
        ),
        (
            without(states(), &format!("{stairs}/defaults/shape")),
            "`defaults` does not name exactly the block's properties",
        ),
        (
            with(states(), &format!("{stairs}/defaults/axis"), json!("y")),
            "`defaults` does not name exactly the block's properties",
        ),
        (
            with(states(), &format!("{stairs}/dropped"), json!(["axis"])),
            "it drops `axis`, which it does not hold",
        ),
        (
            with(
                states(),
                &format!("{stairs}/states/weirdo_direction/reads"),
                json!(["axis"]),
            ),
            "`weirdo_direction` reads a property it does not hold",
        ),
        (
            without(
                states(),
                &format!("{stairs}/states/weirdo_direction/map/east"),
            ),
            "`weirdo_direction` does not map exactly the values its reads hold",
        ),
        (
            with(
                states(),
                &format!("{stairs}/states/weirdo_direction/map/up"),
                json!(0),
            ),
            "`weirdo_direction` does not map exactly the values its reads hold",
        ),
        (
            with(
                states(),
                &format!("{stairs}/states/weirdo_direction/map/east"),
                json!(1_i64 << 40),
            ),
            "`weirdo_direction` holds an int past `TAG_Int`",
        ),
        (
            with(
                states(),
                &format!("{stairs}/dropped"),
                json!(["shape", "half"]),
            ),
            "it drops `half`, which a state reads",
        ),
        (
            with(states(), &format!("{stairs}/dropped"), json!([])),
            "it holds `shape` at several values, which no state reads and it does not drop",
        ),
        (
            with(states(), &format!("{stairs}/mc_versions"), json!([])),
            "it maps no pair or no version",
        ),
    ];
    for (catalog, problem) in cases {
        assert_eq!(
            load(catalog, sensitivity()).unwrap_err(),
            StatesError::MalformedFamily {
                pair: "oak_stairs".to_owned(),
                problem: problem.to_owned(),
            },
        );
    }
}

#[test]
fn a_family_that_maps_no_pair_is_named_by_its_place() {
    assert_eq!(
        load(with(states(), "/families/3/ids", json!([])), sensitivity()).unwrap_err(),
        StatesError::MalformedFamily {
            pair: "families[3]".to_owned(),
            problem: "it maps no pair or no version".to_owned(),
        },
    );
}

#[test]
fn two_families_of_one_java_block_with_other_properties_or_defaults_disagree() {
    let disagree = StatesError::Disagree {
        mc_version: "1.0".to_owned(),
        java: "minecraft:oak_slab".to_owned(),
        first: "minecraft:oak_slab".to_owned(),
        second: "minecraft:oak_double_slab".to_owned(),
    };
    let defaults = with(states(), "/families/2/defaults/type", json!("double"));
    assert_eq!(load(defaults, sensitivity()).unwrap_err(), disagree);
    let keys = without(
        without(states(), "/families/2/java/waterlogged"),
        "/families/2/defaults/waterlogged",
    );
    assert_eq!(load(keys, sensitivity()).unwrap_err(), disagree);
}

#[test]
fn two_families_of_one_java_block_holding_one_state_overlap() {
    let catalog = with(
        with(states(), "/families/2/java/type", json!(["double", "top"])),
        "/families/2/states/minecraft:vertical_half",
        json!({ "reads": ["type"], "map": { "double": "bottom", "top": "top" } }),
    );
    assert_eq!(
        load(catalog, sensitivity()).unwrap_err(),
        StatesError::Overlap {
            mc_version: "1.0".to_owned(),
            java: "minecraft:oak_slab".to_owned(),
            first: "minecraft:oak_slab".to_owned(),
            second: "minecraft:oak_double_slab".to_owned(),
        },
    );
}

#[test]
fn a_dropped_property_needs_one_classification_per_block() {
    assert_eq!(
        load(states(), without(sensitivity(), "/dropped_states/shape")).unwrap_err(),
        StatesError::Unclassified {
            key: "shape".to_owned(),
            java: "minecraft:oak_stairs".to_owned(),
        },
    );
    let named_elsewhere = with(
        sensitivity(),
        "/dropped_states/shape",
        json!([{ "java": ["birch_stairs"], "kind": "missing_state", "reason": "r" }]),
    );
    assert_eq!(
        load(states(), named_elsewhere).unwrap_err(),
        StatesError::Unclassified {
            key: "shape".to_owned(),
            java: "minecraft:oak_stairs".to_owned(),
        },
    );
    let twice = StatesError::ClassifiedTwice {
        key: "shape".to_owned(),
    };
    let two_defaults = with(
        sensitivity(),
        "/dropped_states/shape",
        json!([
            { "kind": "missing_state", "reason": "r" },
            { "kind": "derived", "reason": "r" }
        ]),
    );
    assert_eq!(load(states(), two_defaults).unwrap_err(), twice);
    let one_block_twice = with(
        sensitivity(),
        "/dropped_states/shape",
        json!([
            { "java": ["oak_stairs"], "kind": "missing_state", "reason": "r" },
            { "java": ["minecraft:oak_stairs"], "kind": "derived", "reason": "r" }
        ]),
    );
    assert_eq!(load(states(), one_block_twice).unwrap_err(), twice);
}

#[test]
fn a_java_block_of_the_same_name_fills_its_defaults() {
    assert_eq!(
        resolve("1.0", "minecraft:oak_stairs", &[]),
        Ok(form(
            "minecraft:oak_stairs",
            &[
                ("upside_down_bit", StateValue::Bool(false)),
                ("weirdo_direction", StateValue::Int(3)),
            ],
        )),
    );
    assert_eq!(
        resolve(
            "1.0",
            "minecraft:oak_stairs",
            &[("half", "top"), ("facing", "east")]
        ),
        Ok(form(
            "minecraft:oak_stairs",
            &[
                ("upside_down_bit", StateValue::Bool(true)),
                ("weirdo_direction", StateValue::Int(0)),
            ],
        )),
    );
}

#[test]
fn a_state_picks_the_family_and_so_the_bedrock_id() {
    assert_eq!(
        resolve("1.0", "minecraft:oak_slab", &[("type", "double")]),
        Ok(form(
            "minecraft:oak_double_slab",
            &[("minecraft:vertical_half", s("bottom"))],
        )),
    );
    assert_eq!(
        resolve("1.0", "minecraft:oak_slab", &[("type", "top")]),
        Ok(form(
            "minecraft:oak_slab",
            &[("minecraft:vertical_half", s("top"))],
        )),
    );
}

#[test]
fn waterlogged_is_the_water_layer_not_a_state() {
    let mut expected = form(
        "minecraft:oak_slab",
        &[("minecraft:vertical_half", s("bottom"))],
    );
    expected.waterlogged = true;
    assert_eq!(
        resolve("1.0", "minecraft:oak_slab", &[("waterlogged", "true")]),
        Ok(expected),
    );
}

#[test]
fn a_missing_state_off_its_default_is_dropped_with_its_reason() {
    let mut expected = form(
        "minecraft:oak_stairs",
        &[
            ("upside_down_bit", StateValue::Bool(false)),
            ("weirdo_direction", StateValue::Int(3)),
        ],
    );
    assert_eq!(
        resolve("1.0", "minecraft:oak_stairs", &[("shape", "straight")]),
        Ok(expected.clone()),
    );
    expected.dropped = vec![DroppedState {
        key: "shape".to_owned(),
        value: "outer_left".to_owned(),
        reason: "stairs render straight".to_owned(),
    }];
    assert_eq!(
        resolve("1.0", "minecraft:oak_stairs", &[("shape", "outer_left")]),
        Ok(expected),
    );
}

#[test]
fn a_derived_state_the_entry_names_is_dropped_as_rederived() {
    let mut expected = form("minecraft:oak_fence", &[]);
    assert_eq!(
        resolve("1.0", "minecraft:oak_fence", &[]),
        Ok(expected.clone())
    );
    expected.rederived = ["east", "north"]
        .map(|key| DroppedState {
            key: key.to_owned(),
            value: "true".to_owned(),
            reason: "connections follow the neighbours".to_owned(),
        })
        .to_vec();
    assert_eq!(
        resolve(
            "1.0",
            "minecraft:oak_fence",
            &[("east", "true"), ("north", "true")]
        ),
        Ok(expected),
    );
}

#[test]
fn a_row_naming_the_block_outranks_the_default_row() {
    let catalog = with(
        sensitivity(),
        "/dropped_states/shape",
        json!([
            { "kind": "missing_state", "reason": "stairs render straight" },
            { "java": ["oak_stairs"], "kind": "derived", "reason": "these stairs connect" }
        ]),
    );
    let index = load(states(), catalog).expect("the fixture loads");
    let resolved = resolve_in(
        &index,
        "1.0",
        "minecraft:oak_stairs",
        &[("shape", "outer_left")],
    )
    .expect("stairs");
    assert_eq!(resolved.dropped, []);
}

#[test]
fn a_property_or_value_the_java_block_lacks_is_a_leak() {
    assert_eq!(
        resolve("1.0", "minecraft:oak_stairs", &[("axis", "x")]),
        Err(StateRefusal::UnknownProperty {
            key: "axis".to_owned(),
            valid: names(&["facing", "half", "shape", "waterlogged"]),
        }),
    );
    assert_eq!(
        resolve("1.0", "minecraft:oak_stairs", &[("facing", "up")]),
        Err(StateRefusal::UnknownValue {
            key: "facing".to_owned(),
            value: "up".to_owned(),
            valid: names(&["north", "south", "west", "east"]),
        }),
    );
    assert_eq!(
        resolve("1.0", "minecraft:oak_slab", &[("type", "sideways")]),
        Err(StateRefusal::UnknownValue {
            key: "type".to_owned(),
            value: "sideways".to_owned(),
            valid: names(&["top", "bottom", "double"]),
        }),
    );
}

#[test]
fn a_pair_its_palette_refuses_is_unmapped_even_written_bare() {
    assert_eq!(
        resolve("1.0", "minecraft:jigsaw", &[]),
        Err(palette_refused("minecraft:jigsaw"))
    );
    assert_eq!(
        resolve("1.0", "minecraft:jigsaw", &[("orientation", "north_up")]),
        Err(palette_refused("minecraft:jigsaw")),
    );
    // Refused as `skull=creeper_head`: the Java id reads as itself though Bedrock spells it apart.
    assert_eq!(
        resolve("1.0", "minecraft:creeper_head", &[]),
        Err(palette_refused("minecraft:creeper_head")),
    );
    assert_eq!(
        resolve("1.0", "minecraft:creeper_head", &[("rotation", "4")]),
        Err(palette_refused("minecraft:creeper_head")),
    );
}

#[test]
fn a_state_no_family_holds_on_a_partly_refused_block_is_unmapped() {
    // `oak_fence` maps onto itself, while its pair onto `oak_double_slab` is refused.
    let partly = with(
        states(),
        "/refused",
        json!({ "1.0": ["jigsaw", "skull=creeper_head", "oak_double_slab=oak_fence"] }),
    );
    let index = load(partly, sensitivity()).expect("a partly refused block loads");
    assert_eq!(
        resolve_in(&index, "1.0", "minecraft:oak_fence", &[]),
        Ok(form("minecraft:oak_fence", &[])),
    );
    assert_eq!(
        resolve_in(&index, "1.0", "minecraft:oak_fence", &[("bogus", "1")]),
        Err(palette_refused("minecraft:oak_fence")),
    );
    assert_eq!(
        resolve_in(
            &index,
            "1.0",
            "minecraft:oak_double_slab",
            &[("type", "double")]
        ),
        Ok(form(
            "minecraft:oak_double_slab",
            &[("minecraft:vertical_half", s("bottom"))],
        )),
    );
    assert_eq!(
        resolve_in(
            &index,
            "1.0",
            "minecraft:oak_double_slab",
            &[("bogus", "1")]
        ),
        Err(palette_refused("minecraft:oak_double_slab")),
    );
    let at = index.at("1.0").expect("the fixture maps 1.0");
    assert_eq!(at.canonical_domain("minecraft:oak_fence"), None);
    assert_eq!(at.canonical_domain("minecraft:oak_double_slab"), None);
}

#[test]
fn a_state_only_another_versions_map_holds_is_unmapped_there() {
    assert_eq!(
        resolve("1.0", "minecraft:cherry_wood", &[("axis", "x")]),
        Err(unmapped(
            "its state map there is read from a Java release whose `minecraft:cherry_wood` has \
             no `axis=x`, which the map of 1.1 holds"
        )),
    );
    assert_eq!(
        resolve("1.0", "minecraft:cherry_wood", &[("waterlogged", "true")]),
        Err(unmapped(
            "its state map there is read from a Java release whose `minecraft:cherry_wood` has \
             no `waterlogged`, which the map of 1.1 holds"
        )),
    );
    assert_eq!(
        resolve("1.0", "minecraft:cherry_wood", &[]),
        Ok(form("minecraft:cherry_wood", &[("pillar_axis", s("y"))])),
    );
    assert_eq!(
        resolve("1.1", "minecraft:cherry_wood", &[("axis", "x")]),
        Ok(form("minecraft:cherry_wood", &[("pillar_axis", s("x"))])),
    );
    let index = load(states(), sensitivity()).expect("the fixture loads");
    let sideways = BlockState {
        id: "minecraft:cherry_wood".to_owned(),
        properties: [("axis".to_owned(), "x".to_owned())].into_iter().collect(),
    };
    assert_eq!(
        index.versions_writing(&sideways).collect::<Vec<_>>(),
        ["1.1"]
    );
}

#[test]
fn a_block_that_always_stands_in_water_holds_water() {
    let mut expected = form("minecraft:kelp", &[("kelp_age", StateValue::Int(1))]);
    expected.waterlogged = true;
    assert_eq!(
        resolve("1.0", "minecraft:kelp", &[("age", "1")]),
        Ok(expected)
    );
}

#[test]
fn values_no_one_family_holds_together_are_unmapped() {
    assert_eq!(
        resolve(
            "1.0",
            "minecraft:birch_slab",
            &[("type", "top"), ("waterlogged", "true")]
        ),
        Err(unmapped(
            "no family of `minecraft:birch_slab` there holds these states together, so the pack \
             carries no form for them"
        )),
    );
}

#[test]
fn a_bedrock_spelling_whose_java_default_it_does_not_hold_is_written_bare() {
    assert_eq!(
        resolve("1.0", "minecraft:wall_banner", &[]),
        Ok(form("minecraft:wall_banner", &[])),
    );
    assert_eq!(
        resolve("1.0", "minecraft:wall_banner", &[("facing", "west")]),
        Ok(form(
            "minecraft:wall_banner",
            &[("facing_direction", StateValue::Int(2))]
        )),
    );
}

#[test]
fn a_bedrock_spelling_whose_families_each_take_part_of_the_states_needs_them_to_choose() {
    assert_eq!(
        resolve("1.0", "minecraft:bed", &[("part", "head")]),
        Ok(form(
            "minecraft:bed",
            &[("head_piece_bit", StateValue::Bool(true))]
        )),
    );
    assert_eq!(
        resolve(
            "1.0",
            "minecraft:bed",
            &[("part", "head"), ("occupied", "true")]
        ),
        Err(unmapped(
            "`minecraft:bed` is `minecraft:white_bed` or `minecraft:red_bed` there, and these \
             states do not say which"
        )),
    );
}

#[test]
fn a_bedrock_block_no_java_block_maps_onto_is_written_bare_or_refused() {
    assert_eq!(
        resolve("1.0", "minecraft:frame", &[]),
        Ok(form("minecraft:frame", &[])),
    );
    assert_eq!(
        resolve("1.0", "minecraft:frame", &[("facing", "north")]),
        Err(unmapped(
            "no Java block maps onto `minecraft:frame` there, so the canonical vocabulary names \
             none of its states"
        )),
    );
}

#[test]
fn a_bedrock_spelling_takes_the_states_of_the_java_block_mapped_onto_it() {
    assert_eq!(
        resolve("1.0", "minecraft:oak_double_slab", &[]),
        Ok(form(
            "minecraft:oak_double_slab",
            &[("minecraft:vertical_half", s("bottom"))],
        )),
    );
}

#[test]
fn java_blocks_that_map_alike_onto_one_spelling_resolve_as_one() {
    assert_eq!(
        resolve("1.0", "minecraft:standing_sign", &[("rotation", "2")]),
        Ok(form(
            "minecraft:standing_sign",
            &[("ground_sign_direction", StateValue::Int(2))],
        )),
    );
    assert_eq!(
        resolve("1.0", "minecraft:standing_sign", &[]),
        Ok(form(
            "minecraft:standing_sign",
            &[("ground_sign_direction", StateValue::Int(0))],
        )),
    );
}

#[test]
fn java_blocks_that_map_apart_onto_one_spelling_need_the_states_to_choose() {
    assert_eq!(
        resolve("1.0", "minecraft:log", &[]),
        Ok(form("minecraft:log", &[])),
    );
    assert_eq!(
        resolve("1.0", "minecraft:log", &[("axis", "x")]),
        Err(unmapped(
            "`minecraft:log` is `minecraft:oak_log` or `minecraft:birch_log` there, and these \
             states do not say which"
        )),
    );
    assert_eq!(
        resolve("1.0", "minecraft:log", &[("axis", "w")]),
        Err(StateRefusal::UnknownValue {
            key: "axis".to_owned(),
            value: "w".to_owned(),
            valid: names(&["x", "y", "z"]),
        }),
    );
    assert_eq!(
        resolve("1.0", "minecraft:stonebrick", &[("variant", "mossy")]),
        Err(StateRefusal::UnknownProperty {
            key: "variant".to_owned(),
            valid: Vec::new(),
        }),
    );
}

#[test]
fn several_java_blocks_are_listed_with_a_count_past_four() {
    assert_eq!(and_or_list(&["a"]), "`a`");
    assert_eq!(and_or_list(&["a", "b"]), "`a` or `b`");
    assert_eq!(and_or_list(&["a", "b", "c", "d"]), "`a`, `b`, `c` or `d`");
    assert_eq!(
        and_or_list(&["a", "b", "c", "d", "e"]),
        "`a`, `b`, `c` or 2 other Java blocks",
    );
}

#[test]
fn a_java_block_is_written_apart_only_where_its_forms_are_its_own() {
    let index = load(states(), sensitivity()).expect("the fixture loads");
    let at = index.at("1.0").expect("the fixture maps 1.0");
    assert_eq!(
        at.written_apart("minecraft:stone_bricks"),
        Some("minecraft:stonebrick")
    );
    assert_eq!(
        at.written_apart("minecraft:birch_log"),
        Some("minecraft:log")
    );
    assert_eq!(
        at.written_apart("minecraft:oak_sign"),
        None,
        "`spruce_sign` writes the same forms"
    );
    assert_eq!(
        at.written_apart("minecraft:oak_slab"),
        None,
        "a Bedrock spelling is written as itself"
    );
    assert_eq!(
        at.written_apart("minecraft:creeper_head"),
        None,
        "refused here"
    );
    assert_eq!(at.written_apart("minecraft:no_such_block"), None);
}

#[test]
fn a_java_block_sharing_one_form_or_refused_one_pair_is_not_written_apart() {
    let apart = |catalog: Value, java: &str| {
        let index = load(catalog, sensitivity()).expect("the variant loads");
        let at = index.at("1.0").expect("the fixture maps 1.0");
        at.written_apart(java).map(str::to_owned)
    };
    assert_eq!(
        apart(states(), "minecraft:white_bed"),
        Some("minecraft:bed".to_owned())
    );
    let mut shared = states();
    let red_bed = shared["families"]
        .as_array_mut()
        .and_then(|families| {
            families
                .iter_mut()
                .find(|f| f["ids"] == json!(["bed=red_bed"]))
        })
        .expect("the fixture maps `red_bed`");
    red_bed["java"] = json!({ "occupied": ["true"], "part": ["head", "foot"] });
    red_bed["states"] =
        json!({ "head_piece_bit": { "reads": ["part"], "map": { "head": true, "foot": false } } });
    assert_eq!(apart(shared.clone(), "minecraft:red_bed"), None);
    assert_eq!(apart(shared, "minecraft:white_bed"), None);
    let refused = with(
        states(),
        "/refused",
        json!({ "1.0": ["jigsaw", "skull=creeper_head", "frame=white_bed"] }),
    );
    assert_eq!(apart(refused, "minecraft:white_bed"), None);
}
