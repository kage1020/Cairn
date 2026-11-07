"""Tests for extract_registry.py's state maps, run as `python3 -m unittest discover -s .github/scripts`."""

from __future__ import annotations

import io
import itertools
import json
import unittest
import zipfile

import extract_registry as extract
from minecraft_load import Byte, Refused


def report(blocks: dict[str, tuple[dict[str, list[str]], dict[str, str]]]) -> dict:
    """A `--reports` `blocks.json` holding every state of `blocks`, each its domains and default."""
    out = {}
    next_id = 0
    for name, (domains, default) in blocks.items():
        states = []
        keys = list(domains)
        for values in itertools.product(*(domains[k] for k in keys)):
            properties = dict(zip(keys, values))
            state = {"id": next_id, "properties": properties} if keys else {"id": next_id}
            if properties == default:
                state["default"] = True
            states.append(state)
            next_id += 1
        block = {"states": states}
        if keys:
            block["properties"] = domains
        out[f"minecraft:{name}"] = block
    return out


STAIR_DOMAINS = {
    "facing": ["north", "south"],
    "half": ["top", "bottom"],
    "shape": ["straight", "outer_left"],
    "waterlogged": ["true", "false"],
}
STAIR_DEFAULT = {"facing": "north", "half": "bottom", "shape": "straight", "waterlogged": "false"}
JAVA = extract.java_states(
    report({"oak_stairs": (STAIR_DOMAINS, STAIR_DEFAULT), "oak_planks": ({}, {})})
)


def stair_mapping(properties: dict[str, str]) -> dict:
    """Geyser's entry for a Java stair state: `shape` and `waterlogged` read by no state."""
    return {
        "bedrock_identifier": "minecraft:oak_stairs",
        "state": {
            "upside_down_bit": Byte(1 if properties["half"] == "top" else 0),
            "weirdo_direction": 3 if properties["facing"] == "north" else 2,
        },
    }


def mappings() -> list[dict]:
    """Geyser's entries for every state of `JAVA`, in id order."""
    out = []
    for name, properties in JAVA.rows:
        if name == "oak_stairs":
            out.append(stair_mapping(properties))
        else:
            out.append({"bedrock_identifier": "oak_planks"})
    return out


PALETTE = {
    "oak_stairs": {"upside_down_bit": [False, True], "weirdo_direction": [0, 1, 2, 3]},
    "oak_planks": {},
}


class DecidedByTest(unittest.TestCase):
    def test_a_state_is_read_from_the_fewest_properties_that_decide_it(self):
        rows = [
            ({"facing": f, "half": h}, {"bit": h == "top"})
            for f in ("north", "south")
            for h in ("top", "bottom")
        ]
        rule = extract.decided_by(rows, ["facing", "half"], "bit")
        self.assertEqual(rule, extract.StateRule(("half",), (("bottom", False), ("top", True))))

    def test_a_state_every_row_shares_is_a_constant(self):
        rows = [({"facing": f}, {"bit": True}) for f in ("north", "south")]
        self.assertEqual(
            extract.decided_by(rows, ["facing"], "bit"), extract.StateRule((), (("", True),))
        )

    def test_a_state_some_row_lacks_is_decided_by_nothing(self):
        rows = [({"facing": "north"}, {"bit": True}), ({"facing": "south"}, {})]
        self.assertIsNone(extract.decided_by(rows, ["facing"], "bit"))

    def test_a_state_no_property_decides_is_decided_by_nothing(self):
        rows = [({"facing": "north"}, {"bit": True}), ({"facing": "north"}, {"bit": False})]
        self.assertIsNone(extract.decided_by(rows, ["facing"], "bit"))


class FamilyTest(unittest.TestCase):
    def test_a_property_no_state_reads_is_dropped_but_the_water_layer(self):
        rows = [(p, stair_mapping(p)["state"]) for name, p in JAVA.rows if name == "oak_stairs"]
        rows = [(p, {k: extract.bedrock_value(v) for k, v in s.items()}) for p, s in rows]
        family = extract.family_of("oak_stairs", rows, JAVA)
        self.assertEqual(family.dropped, ("shape",))
        self.assertEqual(
            [(state, rule.reads) for state, rule in family.states],
            [("upside_down_bit", ("half",)), ("weirdo_direction", ("facing",))],
        )
        self.assertEqual(family.java[0], ("facing", ("north", "south")))

    def test_states_that_are_no_product_of_their_values_are_refused(self):
        rows = [
            ({"facing": "north", "half": "top", "shape": "straight", "waterlogged": "false"}, {}),
            ({"facing": "south", "half": "bottom", "shape": "straight", "waterlogged": "false"}, {}),
        ]
        with self.assertRaisesRegex(Refused, "no product of their values"):
            extract.family_of("oak_stairs", rows, JAVA)


class VersionFamiliesTest(unittest.TestCase):
    def test_each_pair_the_palette_accepts_is_one_family(self):
        version_map = extract.version_families(JAVA, mappings(), PALETTE)
        self.assertEqual(
            sorted(version_map.families), [("oak_planks", "oak_planks"), ("oak_stairs", "oak_stairs")]
        )
        self.assertEqual(version_map.refused, [])

    def test_a_pair_the_palette_does_not_declare_is_refused(self):
        palette = {**PALETTE, "oak_stairs": {"upside_down_bit": [False, True], "weirdo_direction": [0]}}
        version_map = extract.version_families(JAVA, mappings(), palette)
        self.assertEqual(sorted(version_map.families), [("oak_planks", "oak_planks")])
        self.assertEqual(version_map.refused, ["oak_stairs"])

    def test_geysers_placeholder_maps_nothing(self):
        entries = mappings()
        entries[-1] = {"bedrock_identifier": "minecraft:unknown"}
        version_map = extract.version_families(JAVA, entries, PALETTE)
        self.assertEqual(sorted(version_map.families), [("oak_stairs", "oak_stairs")])
        self.assertEqual(version_map.refused, [])

    def test_a_map_whose_length_is_not_the_java_states_is_refused(self):
        with self.assertRaisesRegex(Refused, "entries for"):
            extract.version_families(JAVA, mappings()[:-1], PALETTE)


SHAPE = {"kind": "missing_state", "reason": "stairs render straight"}


class SensitivityTest(unittest.TestCase):
    def maps(self):
        return {"1.0": extract.version_families(JAVA, mappings(), PALETTE)}

    def check(self, dropped_states: dict) -> None:
        extract.check_sensitivity(self.maps(), {"dropped_states": dropped_states})

    def test_every_dropped_state_classified_once_passes(self):
        self.check({"shape": [SHAPE]})
        self.check({"shape": [{**SHAPE, "java": ["oak_stairs"]}]})

    def test_a_dropped_state_without_a_row_is_refused(self):
        with self.assertRaisesRegex(Refused, "`shape` on `oak_stairs` is unclassified"):
            self.check({})

    def test_a_row_nothing_drops_is_refused(self):
        with self.assertRaisesRegex(Refused, "no state map drops `shape` on `birch_stairs`"):
            self.check({"shape": [SHAPE, {**SHAPE, "java": ["birch_stairs"]}]})

    def test_a_block_classified_twice_is_refused(self):
        with self.assertRaisesRegex(Refused, "`shape` classifies a block twice"):
            self.check({"shape": [SHAPE, SHAPE]})

    def test_an_unnamed_row_left_covering_nothing_is_refused(self):
        with self.assertRaisesRegex(Refused, "`shape`'s unnamed row covers no block"):
            self.check({"shape": [SHAPE, {**SHAPE, "java": ["oak_stairs"]}]})


class StatesTextTest(unittest.TestCase):
    def test_a_family_two_versions_share_is_one_row_and_refusals_are_per_version(self):
        mapped = extract.version_families(JAVA, mappings(), PALETTE)
        refusing = extract.version_families(
            JAVA, mappings(), {**PALETTE, "oak_stairs": {"upside_down_bit": [False]}}
        )
        text = extract.states_text(
            {"1.0": mapped, "1.1": mapped, "1.2": refusing},
            {"dropped_states": {"shape": [SHAPE]}},
        )
        body = json.loads(text)
        self.assertEqual(body["refused"], {"1.2": ["oak_stairs"]})
        self.assertEqual(
            [(row["mc_versions"], row["ids"]) for row in body["families"]],
            [(["1.0", "1.1", "1.2"], ["oak_planks"]), (["1.0", "1.1"], ["oak_stairs"])],
        )
        stairs = body["families"][1]
        self.assertEqual(stairs["dropped"], ["shape"])
        self.assertEqual(
            stairs["states"]["weirdo_direction"],
            {"reads": ["facing"], "map": {"north": 3, "south": 2}},
        )
        self.assertEqual(body["families"][0]["states"], {})



def archive(files: dict[str, object]) -> zipfile.ZipFile:
    """An in-memory jar holding each of `files` as JSON."""
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as out:
        for name, body in files.items():
            out.writestr(name, json.dumps(body))
    return zipfile.ZipFile(buffer)


# A stair facing east as the game's model draws it: a bottom slab and the upper step at the back.
STAIR_MODEL = {
    "elements": [
        {"from": [0, 0, 0], "to": [16, 8, 16], "faces": {f: {} for f in extract.FACES}},
        {"from": [8, 8, 0], "to": [16, 16, 16], "faces": {f: {} for f in extract.FACES}},
    ]
}


class WholeFacesTest(unittest.TestCase):
    def test_a_face_two_boxes_cover_between_them_is_whole(self):
        boxes = extract.placed_boxes(extract.model_boxes(archive({
            "assets/minecraft/models/block/stairs.json": STAIR_MODEL,
        }), "block/stairs", {}), 0, 0)
        self.assertEqual(extract.whole_faces(boxes), {"down", "east"})

    def test_a_turned_model_turns_its_whole_faces(self):
        client = archive({"assets/minecraft/models/block/stairs.json": STAIR_MODEL})
        boxes = extract.model_boxes(client, "block/stairs", {})
        # A quarter turn about y takes east to south; a half turn about x takes down to up.
        self.assertEqual(extract.whole_faces(extract.placed_boxes(boxes, 0, 90)), {"down", "south"})
        self.assertEqual(extract.whole_faces(extract.placed_boxes(boxes, 180, 0)), {"up", "east"})

    def test_a_box_that_shows_no_face_there_or_is_turned_by_an_angle_covers_nothing(self):
        client = archive({
            "assets/minecraft/models/block/hollow.json": {"elements": [
                {"from": [0, 0, 0], "to": [16, 16, 16], "faces": {"up": {}, "down": {}}},
                {"from": [0, 0, 0], "to": [16, 16, 16], "faces": {"north": {}},
                 "rotation": {"origin": [8, 8, 8], "axis": "y", "angle": 45}},
            ]},
        })
        boxes = extract.placed_boxes(extract.model_boxes(client, "block/hollow", {}), 0, 0)
        self.assertEqual(extract.whole_faces(boxes), {"up", "down"})

    def test_a_square_left_open_by_a_sliver_is_not_covered(self):
        self.assertTrue(extract.covers_square([(0, 8.5, 0, 16), (8.5, 16, 0, 16)]))
        self.assertFalse(extract.covers_square([(0, 8, 0, 16), (8.5, 16, 0, 16)]))


class StateModelsTest(unittest.TestCase):
    def test_a_variant_is_the_one_whose_key_the_state_holds(self):
        blockstate = {"variants": {"type=bottom": {"model": "a"}, "type=top": {"model": "b", "x": 180}}}
        self.assertEqual(
            extract.state_models(blockstate, {"type": "top", "waterlogged": "false"}),
            [[{"model": "b", "x": 180}]],
        )

    def test_a_multipart_draws_every_part_whose_condition_holds(self):
        blockstate = {"multipart": [
            {"apply": {"model": "post"}},
            {"apply": {"model": "side"}, "when": {"north": "low|tall"}},
            {"apply": {"model": "cap"}, "when": {"OR": [{"up": "true"}, {"AND": [{"east": "!none"}]}]}},
        ]}
        self.assertEqual(
            extract.state_models(blockstate, {"north": "tall", "east": "low", "up": "false"}),
            [[{"model": "post"}, {"model": "side"}, {"model": "cap"}]],
        )
        self.assertEqual(
            extract.state_models(blockstate, {"north": "none", "east": "none", "up": "false"}),
            [[{"model": "post"}]],
        )


class TagMembersTest(unittest.TestCase):
    def test_a_tag_follows_its_nested_tags_and_reads_each_feature_packs_additions(self):
        server = archive({
            "data/minecraft/tags/blocks/walls.json": {"values": ["minecraft:cobblestone_wall", "#minecraft:extra"]},
            "data/minecraft/tags/blocks/extra.json": {"values": [{"id": "minecraft:brick_wall", "required": False}]},
            "data/minecraft/datapacks/update_1_21/data/minecraft/tags/blocks/walls.json": {
                "values": ["minecraft:tuff_wall"]
            },
        })
        self.assertEqual(
            extract.tag_members(server, "walls"), {"cobblestone_wall", "brick_wall", "tuff_wall"}
        )

    def test_a_tag_a_feature_pack_replaces_is_refused(self):
        server = archive({
            "data/minecraft/tags/block/walls.json": {"values": []},
            "data/minecraft/datapacks/x/data/minecraft/tags/block/walls.json": {"replace": True, "values": []},
        })
        with self.assertRaises(Refused):
            extract.tag_members(server, "walls")


TAGS = {
    "walls": {"cobblestone_wall"},
    "fences": {"oak_fence", "nether_brick_fence"},
    "wooden_fences": {"oak_fence"},
    "fence_gates": set(),
}
FENCE_STATES = {**extract.KIND_STATES["fence"], "waterlogged": extract.BOOLEAN}


class BlockKindTest(unittest.TestCase):
    def test_a_pane_is_a_block_with_a_panes_states_that_is_no_fence(self):
        self.assertEqual(extract.block_kind("glass_pane", FENCE_STATES, TAGS), "pane")
        self.assertEqual(extract.block_kind("oak_fence", FENCE_STATES, TAGS), "wooden_fence")
        self.assertEqual(extract.block_kind("nether_brick_fence", FENCE_STATES, TAGS), "fence")
        self.assertIsNone(extract.block_kind("stone", {}, TAGS))

    def test_a_chest_is_a_block_whose_type_pairs_it(self):
        chest = {**extract.KIND_STATES["chest"], "waterlogged": extract.BOOLEAN}
        self.assertEqual(extract.block_kind("trapped_chest", chest, TAGS), "chest")
        ender = {"facing": chest["facing"], "waterlogged": extract.BOOLEAN}
        self.assertIsNone(extract.block_kind("ender_chest", ender, TAGS))
        slab = {"type": ["top", "bottom", "double"], "waterlogged": extract.BOOLEAN}
        self.assertIsNone(extract.block_kind("oak_slab", slab, TAGS))

    def test_a_tagged_block_without_its_kinds_states_is_refused(self):
        with self.assertRaises(Refused):
            extract.block_kind("cobblestone_wall", FENCE_STATES, TAGS)


class ConnectionsTextTest(unittest.TestCase):
    def test_a_block_read_alike_at_two_versions_is_one_row_with_both(self):
        wall = extract.Joins("wall", False, False, None)
        slab = extract.Joins(
            None, False, False, extract.Sturdy(("type",), (("bottom", ("down",)), ("top", ("up",)))),
            (("type", "bottom"),),
        )
        body = json.loads(extract.connections_text({
            "1.0": {"cobblestone_wall": wall, "oak_slab": slab},
            "1.1": {"cobblestone_wall": wall, "tuff_wall": wall},
        }))
        self.assertEqual(body["rows"], [
            {"mc_versions": ["1.0", "1.1"], "ids": ["cobblestone_wall"], "kind": "wall"},
            {"mc_versions": ["1.0"], "ids": ["oak_slab"], "defaults": {"type": "bottom"},
             "sturdy": {"reads": ["type"], "map": {"bottom": ["down"], "top": ["up"]}}},
            {"mc_versions": ["1.1"], "ids": ["tuff_wall"], "kind": "wall"},
        ])


if __name__ == "__main__":
    unittest.main()
