"""Tests for extract_registry.py's state maps, run as `python3 -m unittest discover -s .github/scripts`."""

from __future__ import annotations

import itertools
import json
import unittest

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


if __name__ == "__main__":
    unittest.main()
