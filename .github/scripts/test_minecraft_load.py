"""Tests for minecraft_load.py, run as `python3 -m unittest discover -s .github/scripts`."""

from __future__ import annotations

import gzip
import unittest

import minecraft_load as load


def structure(*blocks: tuple[tuple[int, int, int], dict], data_version: int = 4189) -> dict:
    """A structure holding `blocks`, each a position and a palette entry, one entry per block."""
    return {
        "DataVersion": data_version,
        "size": [2, 1, 1],
        "palette": [entry for _, entry in blocks],
        "blocks": [{"pos": list(pos), "state": i} for i, (pos, _) in enumerate(blocks)],
        "entities": [],
    }


STAIR = {"Name": "minecraft:oak_stairs", "Properties": {"facing": "north", "half": "top"}}
PLANKS = {"Name": "minecraft:oak_planks"}


class Nbt(unittest.TestCase):
    def test_a_structure_reads_back_as_written(self) -> None:
        written = structure(((0, 0, 0), STAIR), ((1, 0, 0), PLANKS))
        self.assertEqual(load.read_nbt(load.write_nbt(written)), written)

    def test_a_file_that_ends_inside_a_tag_is_refused(self) -> None:
        raw = gzip.decompress(load.write_nbt(structure(((0, 0, 0), PLANKS))))
        with self.assertRaisesRegex(ValueError, "NBT ends inside a tag"):
            load.read_nbt(gzip.compress(raw[:-3]))

    def test_a_root_that_is_not_a_compound_is_refused(self) -> None:
        with self.assertRaisesRegex(ValueError, "the root tag is not a compound"):
            load.read_nbt(gzip.compress(bytes([load.TAG_STRING, 0, 0, 0, 0])))


class Differences(unittest.TestCase):
    def differences(self, loaded: dict) -> list[str]:
        written = structure(((0, 0, 0), STAIR), ((1, 0, 0), PLANKS))
        return load.differences(written, loaded)

    def test_the_same_blocks_in_another_palette_order_are_no_difference(self) -> None:
        self.assertEqual(self.differences(structure(((1, 0, 0), PLANKS), ((0, 0, 0), STAIR))), [])

    def test_a_default_property_the_game_saves_is_no_difference(self) -> None:
        stair = {**STAIR, "Properties": {**STAIR["Properties"], "waterlogged": "false"}}
        self.assertEqual(self.differences(structure(((0, 0, 0), stair), ((1, 0, 0), PLANKS))), [])

    def test_an_id_the_game_does_not_know_is_read_as_air(self) -> None:
        loaded = structure(((0, 0, 0), STAIR), ((1, 0, 0), {"Name": "minecraft:air"}))
        self.assertEqual(
            self.differences(loaded), ["(1, 0, 0): minecraft:oak_planks was read as minecraft:air"]
        )

    def test_a_value_outside_the_domain_and_an_unknown_key_are_named(self) -> None:
        stair = {"Name": "minecraft:oak_stairs", "Properties": {"facing": "south"}}
        self.assertEqual(
            self.differences(structure(((0, 0, 0), stair), ((1, 0, 0), PLANKS))),
            [
                "(0, 0, 0): minecraft:oak_stairs[facing=north] read as facing=south",
                "(0, 0, 0): minecraft:oak_stairs[half=top] dropped",
            ],
        )

    def test_an_upgraded_data_version_a_resized_box_and_moved_blocks_are_named(self) -> None:
        loaded = structure(((0, 0, 0), STAIR), ((0, 0, 1), PLANKS), data_version=4200)
        loaded["size"] = [2, 1, 2]
        loaded["entities"] = [{}]
        self.assertEqual(
            self.differences(loaded),
            [
                "DataVersion 4189 was read as 4200",
                "size [2, 1, 1] was read as [2, 1, 2]",
                "0 entities were read as 1",
                "(0, 0, 1): minecraft:oak_planks appeared",
                "(1, 0, 0): minecraft:oak_planks was dropped",
            ],
        )

    def test_a_block_entity_the_game_drops_is_named(self) -> None:
        written = structure(((0, 0, 0), STAIR), ((1, 0, 0), PLANKS))
        written["blocks"][1]["nbt"] = {"id": "minecraft:sign"}
        self.assertEqual(
            load.differences(written, structure(((0, 0, 0), STAIR), ((1, 0, 0), PLANKS))),
            ["(1, 0, 0): minecraft:oak_planks's block entity was dropped"],
        )


class Targets(unittest.TestCase):
    def test_only_targetable_rows_are_checked_and_a_row_that_does_not_say_is_one(self) -> None:
        table = {
            "versions": [
                {"mc_version": "1.20.3", "data_version": 3698, "targetable": False},
                {"mc_version": "1.20.4", "data_version": 3700},
                {"mc_version": "1.21", "data_version": 3953, "targetable": True},
            ]
        }
        self.assertEqual(
            load.shipped_targets(table),
            [load.Target("1.20.4", 3700), load.Target("1.21", 3953)],
        )

    def test_structures_move_to_the_singular_folder_from_1_21(self) -> None:
        folders = [
            load.Target(v, 0).structure_folder() for v in ("1.20.4", "1.20.10", "1.21", "26.1")
        ]
        self.assertEqual(folders, ["structures", "structures", "structure", "structure"])


if __name__ == "__main__":
    unittest.main()
