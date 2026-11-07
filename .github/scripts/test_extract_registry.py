"""Tests for extract_registry.py, run as `python3 -m unittest discover -s .github/scripts`."""

from __future__ import annotations

import contextlib
import hashlib
import io
import itertools
import json
import shutil
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

import extract_registry as registry
import minecraft_load as load


def typed(value) -> str:
    """`value` as JSON, which tells `True` from `1` where `==` does not."""
    return json.dumps(value)


def palette(*entries: tuple[str, dict], version: int = 18_168_865) -> dict:
    """A Bedrock block palette of `entries`, each a block id and one state of it."""
    return {
        "blocks": [
            {"name": name, "states": states, "version": load.Int(version)}
            for name, states in entries
        ]
    }


def table(mc_version: str, blocks: dict, version_int: int = 1) -> registry.Table:
    """A table of `blocks` for `mc_version`, from nowhere in particular."""
    return registry.Table(mc_version, blocks, version_int, {})


class Case(unittest.TestCase):
    def patch(self, target, name: str, value) -> None:
        patcher = mock.patch.object(target, name, value)
        patcher.start()
        self.addCleanup(patcher.stop)

    def temporary_folder(self) -> Path:
        folder = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, folder)
        return folder

    def refusal(self, call, *args) -> str:
        with self.assertRaises(load.Refused) as caught:
            call(*args)
        return str(caught.exception)


class JavaBlocks(Case):
    def test_ids_and_properties_sort_by_name_and_each_domain_keeps_the_games_order(self) -> None:
        report = {
            "minecraft:stone": {"states": [{"id": 1, "default": True}]},
            "minecraft:lever": {
                "properties": {"powered": ["true", "false"], "face": ["floor", "wall", "ceiling"]},
                "states": [],
            },
        }
        self.assertEqual(
            typed(registry.java_blocks(report)),
            typed(
                {
                    "lever": {"face": ["floor", "wall", "ceiling"], "powered": ["true", "false"]},
                    "stone": {},
                }
            ),
        )

    def test_an_empty_report_or_an_id_outside_the_namespace_is_refused(self) -> None:
        self.assertEqual(self.refusal(registry.java_blocks, {}), "the block report lists no block")
        self.assertEqual(
            self.refusal(registry.java_blocks, {"other:stone": {}}),
            "`other:stone` is not a `minecraft:` id",
        )


class BedrockValue(Case):
    def test_a_byte_is_a_boolean_and_an_integer_or_string_stays_itself(self) -> None:
        values = (load.Byte(0), load.Byte(1), load.Int(1), load.Short(3), "north")
        self.assertEqual(
            typed([registry.bedrock_value(value) for value in values]),
            typed([False, True, 1, 3, "north"]),
        )

    def test_a_byte_that_is_no_boolean_or_a_float_is_refused(self) -> None:
        self.assertEqual(
            self.refusal(registry.bedrock_value, load.Byte(2)),
            "a byte state holds 2, which is no boolean",
        )
        self.assertEqual(
            self.refusal(registry.bedrock_value, 1.5),
            "a palette state holds 1.5, which is no bool, int or string",
        )


class BedrockBlocks(Case):
    def test_each_property_lists_the_values_its_states_take_in_palette_order(self) -> None:
        blocks, version = registry.bedrock_blocks(
            palette(
                ("minecraft:lever", {"open_bit": load.Byte(0), "lever_direction": "north"}),
                ("minecraft:lever", {"open_bit": load.Byte(1), "lever_direction": "north"}),
                ("minecraft:lever", {"open_bit": load.Byte(0), "lever_direction": "up_east_west"}),
                ("minecraft:air", {}),
            )
        )
        self.assertEqual(
            (typed(blocks), version),
            (
                typed(
                    {
                        "air": {},
                        "lever": {
                            "lever_direction": ["north", "up_east_west"],
                            "open_bit": [False, True],
                        },
                    }
                ),
                18_168_865,
            ),
        )

    def test_a_boolean_value_and_an_integer_value_stay_apart(self) -> None:
        blocks, _ = registry.bedrock_blocks(
            palette(
                ("minecraft:hopper", {"facing_direction": load.Int(0), "toggle_bit": load.Byte(1)}),
                ("minecraft:hopper", {"facing_direction": load.Int(1), "toggle_bit": load.Int(1)}),
            )
        )
        self.assertEqual(
            typed(blocks),
            typed({"hopper": {"facing_direction": [0, 1], "toggle_bit": [True, 1]}}),
        )

    def test_an_empty_palette_or_one_with_two_block_state_versions_is_refused(self) -> None:
        self.assertEqual(
            self.refusal(registry.bedrock_blocks, {"blocks": []}), "the palette lists no block"
        )
        two = palette(("minecraft:air", {}))
        two["blocks"] += palette(("minecraft:stone", {}), version=1)["blocks"]
        self.assertEqual(
            self.refusal(registry.bedrock_blocks, two),
            "the palette carries 2 block-state versions, not one",
        )


class BlocksText(Case):
    def test_one_table_is_the_base_and_no_diff(self) -> None:
        self.assertEqual(
            registry.blocks_text(
                [table("1.0", {"lever": {"powered": ["true", "false"]}, "stone": {}})]
            ),
            "{\n"
            '  "schema_version": 2,\n'
            '  "namespace": "minecraft",\n'
            '  "base": {\n'
            '    "mc_version": "1.0",\n'
            '    "blocks": {\n'
            '      "lever": {"powered": ["true", "false"]},\n'
            '      "stone": {}\n'
            "    }\n"
            "  },\n"
            '  "diffs": []\n'
            "}\n",
        )

    def test_each_later_table_is_a_diff_of_the_one_before_it(self) -> None:
        first = {"lever": {"powered": ["true", "false"]}, "sand": {}, "stone": {}}
        second = {
            "lever": {"face": ["floor"], "powered": ["true", "false"]},
            "stone": {},
            "tuff": {},
        }
        self.assertEqual(
            registry.blocks_text([table("1.0", first), table("1.1", second), table("1.2", second)]),
            "{\n"
            '  "schema_version": 2,\n'
            '  "namespace": "minecraft",\n'
            '  "base": {\n'
            '    "mc_version": "1.0",\n'
            '    "blocks": {\n'
            '      "lever": {"powered": ["true", "false"]},\n'
            '      "sand": {},\n'
            '      "stone": {}\n'
            "    }\n"
            "  },\n"
            '  "diffs": [\n'
            "    {\n"
            '      "mc_version": "1.1",\n'
            '      "inherits": "1.0",\n'
            '      "added": {\n'
            '        "tuff": {}\n'
            "      },\n"
            '      "changed": {\n'
            '        "lever": {"face": ["floor"], "powered": ["true", "false"]}\n'
            "      },\n"
            '      "removed": ["sand"]\n'
            "    },\n"
            "    {\n"
            '      "mc_version": "1.2",\n'
            '      "inherits": "1.1",\n'
            '      "added": {},\n'
            '      "changed": {},\n'
            '      "removed": []\n'
            "    }\n"
            "  ]\n"
            "}\n",
        )

    def test_a_domain_retyped_between_boolean_and_integer_is_a_change(self) -> None:
        for before, after in (([False, True], [0, 1]), ([0, 1], [False, True])):
            with self.subTest(before=before, after=after):
                text = registry.blocks_text(
                    [
                        table("1.0", {"lever": {"open_bit": before}}),
                        table("1.1", {"lever": {"open_bit": after}}),
                    ]
                )
                self.assertEqual(
                    typed(json.loads(text)["diffs"][0]["changed"]),
                    typed({"lever": {"open_bit": after}}),
                )


class UpdatedDataVersions(Case):
    def test_a_targetable_java_row_takes_the_games_data_version_and_latest_is_the_newest(
        self,
    ) -> None:
        rows = {
            "schema_version": 2,
            "latest": "1.21.4",
            "note": "kept",
            "versions": [
                {
                    "mc_version": "1.20.3",
                    "released": "2023-12-05",
                    "data_version": 3698,
                    "targetable": False,
                },
                {"released": "2024-12-03", "mc_version": "1.21.4", "data_version": 1},
                {"mc_version": "26.3", "released": "2026-10-01"},
            ],
        }
        self.assertEqual(
            typed(
                registry.updated_data_versions(
                    rows, "java", [table("1.21.4", {}, 4189), table("26.3", {}, 5023)]
                )
            ),
            typed(
                {
                    "schema_version": 3,
                    "latest": "26.3",
                    "note": "kept",
                    "versions": [
                        {
                            "mc_version": "1.20.3",
                            "data_version": 3698,
                            "released": "2023-12-05",
                            "targetable": False,
                        },
                        {"mc_version": "1.21.4", "data_version": 4189, "released": "2024-12-03"},
                        {"mc_version": "26.3", "data_version": 5023, "released": "2026-10-01"},
                    ],
                }
            ),
        )

    def test_a_targetable_bedrock_row_takes_the_palettes_block_version_beside_its_key(self) -> None:
        rows = {
            "schema_version": 3,
            "latest": "1.21.0",
            "versions": [
                {"mc_version": "1.20.80", "data_version": 12080, "targetable": False},
                {"mc_version": "1.21.0", "released": "2024-06-13", "data_version": 12100},
            ],
        }
        self.assertEqual(
            typed(
                registry.updated_data_versions(rows, "bedrock", [table("1.21.0", {}, 18_090_528)])
            ),
            typed(
                {
                    "schema_version": 3,
                    "latest": "1.21.0",
                    "versions": [
                        {"mc_version": "1.20.80", "data_version": 12080, "targetable": False},
                        {
                            "mc_version": "1.21.0",
                            "data_version": 12100,
                            "block_version": 18_090_528,
                            "released": "2024-06-13",
                        },
                    ],
                }
            ),
        )


class ExtractBedrock(Case):
    """`extract_bedrock` offline, its one pinned palette served by a fake `fetch`."""

    PALETTE = load.write_nbt(palette(("minecraft:air", {})))
    SHA256 = hashlib.sha256(PALETTE).hexdigest()
    URL = "https://raw.githubusercontent.com/GeyserMC/Geyser/abc/core/src/main/resources/bedrock/p.nbt"

    def setUp(self) -> None:
        self.cache = self.temporary_folder()
        self.cached = self.cache / f"bedrock-1.21.0-{self.SHA256[:12]}.nbt"
        self.fetched: list[str] = []
        self.patch(registry, "BEDROCK_PALETTES", {"1.21.0": ("abc", "p.nbt", self.SHA256)})

    def serve(self, body: bytes) -> None:
        def fetch(url: str, timeout: int) -> bytes:
            self.fetched.append(url)
            return body

        self.patch(registry, "fetch", fetch)

    def test_a_cached_palette_that_matches_its_pin_is_read_without_a_download(self) -> None:
        self.cached.write_bytes(self.PALETTE)
        self.serve(b"")
        self.assertEqual(
            registry.extract_bedrock("1.21.0", self.cache),
            registry.Table(
                "1.21.0", {"air": {}}, 18_168_865, {"url": self.URL, "sha256": self.SHA256}
            ),
        )
        self.assertEqual(self.fetched, [])

    def test_a_cached_palette_that_fails_its_pin_is_downloaded_again(self) -> None:
        self.cached.write_bytes(b"corrupt")
        self.serve(self.PALETTE)
        self.assertEqual(registry.extract_bedrock("1.21.0", self.cache).blocks, {"air": {}})
        self.assertEqual(self.fetched, [self.URL])
        self.assertEqual(self.cached.read_bytes(), self.PALETTE)

    def test_a_download_that_fails_its_pin_is_refused_and_not_cached(self) -> None:
        self.serve(b"corrupt")
        self.assertEqual(
            self.refusal(registry.extract_bedrock, "1.21.0", self.cache),
            f"the Bedrock 1.21.0 palette at {self.URL} does not match its pinned SHA-256",
        )
        self.assertFalse(self.cached.exists())

    def test_a_version_with_no_pinned_palette_is_refused(self) -> None:
        self.assertEqual(
            self.refusal(registry.extract_bedrock, "1.21.1", self.cache),
            "no Bedrock palette is pinned for 1.21.1; add it to BEDROCK_PALETTES",
        )


class GeyserMappings(Case):
    """`geyser_mappings` offline, its one pinned state map served by a fake `fetch`."""

    MAP = load.write_nbt({"bedrock_mappings": [{"bedrock_identifier": "minecraft:air"}]})
    SHA256 = hashlib.sha256(MAP).hexdigest()
    URL = "https://raw.githubusercontent.com/GeyserMC/mappings/abc/blocks.nbt"

    def setUp(self) -> None:
        self.cache = self.temporary_folder()
        self.cached = self.cache / f"geyser-mappings-{self.SHA256[:12]}.nbt"
        self.fetched: list[str] = []
        self.patch(registry, "BEDROCK_STATE_MAPS", {"1.21.0": ("abc", self.SHA256, "1.21")})

    def serve(self, body: bytes) -> None:
        def fetch(url: str, timeout: int) -> bytes:
            self.fetched.append(url)
            return body

        self.patch(registry, "fetch", fetch)

    def test_a_cached_map_that_matches_its_pin_is_read_without_a_download(self) -> None:
        self.cached.write_bytes(self.MAP)
        self.serve(b"")
        self.assertEqual(
            registry.geyser_mappings("1.21.0", self.cache),
            [{"bedrock_identifier": "minecraft:air"}],
        )
        self.assertEqual(self.fetched, [])

    def test_a_cached_map_that_fails_its_pin_is_downloaded_again(self) -> None:
        self.cached.write_bytes(b"corrupt")
        self.serve(self.MAP)
        self.assertEqual(
            registry.geyser_mappings("1.21.0", self.cache),
            [{"bedrock_identifier": "minecraft:air"}],
        )
        self.assertEqual(self.fetched, [self.URL])
        self.assertEqual(self.cached.read_bytes(), self.MAP)

    def test_a_download_that_fails_its_pin_is_refused_and_not_cached(self) -> None:
        self.serve(b"corrupt")
        self.assertEqual(
            self.refusal(registry.geyser_mappings, "1.21.0", self.cache),
            f"the state map at {self.URL} does not match its pinned SHA-256",
        )
        self.assertFalse(self.cached.exists())

    def test_a_map_whose_nbt_does_not_read_is_refused(self) -> None:
        body = load.write_nbt({"other": []})
        pin = ("abc", hashlib.sha256(body).hexdigest(), "1.21")
        self.patch(registry, "BEDROCK_STATE_MAPS", {"1.21.0": pin})
        self.serve(body)
        self.assertEqual(
            self.refusal(registry.geyser_mappings, "1.21.0", self.cache),
            "the state map for Bedrock 1.21.0 does not read: 'bedrock_mappings'",
        )

    def test_a_version_with_no_pinned_map_is_refused(self) -> None:
        self.assertEqual(
            self.refusal(registry.geyser_mappings, "1.21.1", self.cache),
            "no state map is pinned for Bedrock 1.21.1; add it to BEDROCK_STATE_MAPS",
        )


class ExtractJava(Case):
    """`extract_java` offline: Mojang's metadata, the jar and the data generator are faked."""

    DOWNLOAD = {"url": "https://example.invalid/server.jar", "sha1": "0" * 40}

    def setUp(self) -> None:
        self.tmp = self.temporary_folder()
        self.jar = self.tmp / "server.jar"
        with zipfile.ZipFile(self.jar, "w") as archive:
            archive.writestr("version.json", json.dumps({"world_version": 5023}))
        self.report: dict = {"minecraft:stone": {}}
        self.downloaded: list[str] = []
        self.patch(registry, "server_download", lambda version, cache: (self.DOWNLOAD, 25))
        self.patch(registry, "server_jar", self.server_jar)
        self.patch(registry, "data_generator", self.data_generator)

    def server_jar(self, version: str, cache: Path) -> tuple[Path, int]:
        self.downloaded.append(version)
        return self.jar, 25

    def data_generator(self, java, jar, cwd, log, mode: str, *args: str) -> None:
        options = dict(zip(args[::2], args[1::2]))
        reports = Path(options["--output"]) / "reports"
        reports.mkdir(parents=True)
        (reports / "blocks.json").write_text(json.dumps(self.report), encoding="utf-8")

    def extract(self, has_java: int):
        self.patch(registry, "java_major", lambda java: has_java)
        return registry.extract_java("26.3", "java", self.tmp, self.tmp / "work")

    def test_the_table_takes_the_reports_blocks_and_the_jars_data_version(self) -> None:
        self.assertEqual(
            self.extract(25),
            registry.Table("26.3", {"stone": {}}, 5023, self.DOWNLOAD),
        )

    def test_a_java_too_old_for_the_server_is_refused_before_the_jar_is_downloaded(self) -> None:
        self.assertEqual(
            self.refusal(self.extract, 21), "Java 26.3 needs Java 25, and `java` is 21"
        )
        self.assertEqual(self.downloaded, [])

    def test_an_empty_block_report_is_refused(self) -> None:
        self.report = {}
        self.assertEqual(self.refusal(self.extract, 25), "the block report lists no block")


class DiffHead(unittest.TestCase):
    def test_a_short_difference_is_shown_whole(self) -> None:
        self.assertEqual(
            registry.diff_head("a.json", '{\n  "x": 1\n}\n', '{\n  "x": 2\n}\n'),
            [
                "--- a.json (committed)\n",
                "+++ a.json (from the game)\n",
                "@@ -1,3 +1,3 @@\n",
                " {\n",
                '-  "x": 1\n',
                '+  "x": 2\n',
                " }\n",
            ],
        )

    def test_a_long_difference_shows_its_first_lines_and_counts_the_rest(self) -> None:
        committed = "".join(f"{i}\n" for i in range(100))
        wanted = "".join(f"{i}!\n" for i in range(100))
        self.assertEqual(
            registry.diff_head("a.json", committed, wanted),
            [
                "--- a.json (committed)\n",
                "+++ a.json (from the game)\n",
                "@@ -1,100 +1,100 @@\n",
                *(f"-{i}\n" for i in range(37)),
                "... and 163 more lines\n",
            ],
        )

    def test_a_last_line_without_a_newline_stays_its_own_line(self) -> None:
        self.assertEqual(
            registry.diff_head("a.json", "{}", "{}\n"),
            [
                "--- a.json (committed)\n",
                "+++ a.json (from the game)\n",
                "@@ -1 +1 @@\n",
                "-{}\n",
                "+{}\n",
            ],
        )


class Main(Case):
    COMMITTED = '{\n  "x": 1\n}\n'

    def setUp(self) -> None:
        self.root = self.temporary_folder()
        self.file = self.root / "blocks.json"
        self.file.write_text(self.COMMITTED, encoding="utf-8")
        self.patch(registry, "ROOT", self.root)

    def main(self, *argv: str, wanted: str, extract=lambda *args: []) -> tuple[int, str]:
        stderr = io.StringIO()
        with (
            mock.patch.object(registry, "extract", extract),
            mock.patch.object(registry, "pack_files", lambda edition, tables: {self.file: wanted}),
            contextlib.redirect_stderr(stderr),
            contextlib.redirect_stdout(io.StringIO()),
        ):
            code = registry.main(["bedrock", *argv])
        return code, stderr.getvalue()

    def test_check_names_each_differing_file_with_its_diff_and_writes_nothing(self) -> None:
        self.assertEqual(
            self.main("--check", wanted='{\n  "x": 2\n}\n'),
            (
                1,
                "blocks.json differs from what the game says:\n"
                "--- blocks.json (committed)\n"
                "+++ blocks.json (from the game)\n"
                "@@ -1,3 +1,3 @@\n"
                " {\n"
                '-  "x": 1\n'
                '+  "x": 2\n'
                " }\n",
            ),
        )
        self.assertEqual(self.file.read_text(encoding="utf-8"), self.COMMITTED)

    def test_check_passes_when_every_file_holds_what_the_game_says(self) -> None:
        self.assertEqual(self.main("--check", wanted=self.COMMITTED), (0, ""))

    def test_without_check_a_differing_file_is_rewritten(self) -> None:
        self.assertEqual(self.main(wanted='{\n  "x": 2\n}\n'), (0, ""))
        self.assertEqual(self.file.read_text(encoding="utf-8"), '{\n  "x": 2\n}\n')

    def test_a_refusal_fails_the_run_and_writes_nothing(self) -> None:
        def refuse(*args):
            raise load.Refused("no palette")

        self.assertEqual(self.main(wanted="", extract=refuse), (1, "error: no palette\n"))
        self.assertEqual(self.file.read_text(encoding="utf-8"), self.COMMITTED)

    def test_the_work_folder_is_gone_once_the_run_ends(self) -> None:
        folders: list[Path] = []

        def extract(edition: str, java: str, cache: Path, work: Path) -> list:
            self.assertTrue(work.is_dir())
            folders.append(work)
            return []

        self.main("--check", wanted=self.COMMITTED, extract=extract)
        self.assertEqual(len(folders), 1)
        self.assertFalse(folders[0].exists())


# --- the Bedrock state map ----------------------------------------------------------------------


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
JAVA = registry.java_states(
    report({"oak_stairs": (STAIR_DOMAINS, STAIR_DEFAULT), "oak_planks": ({}, {})})
)


def stair_mapping(properties: dict[str, str]) -> dict:
    """Geyser's entry for a Java stair state: `shape` and `waterlogged` read by no state."""
    return {
        "bedrock_identifier": "minecraft:oak_stairs",
        "state": {
            "upside_down_bit": load.Byte(1 if properties["half"] == "top" else 0),
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


BEDROCK_TABLE = {
    "oak_stairs": {"upside_down_bit": [False, True], "weirdo_direction": [0, 1, 2, 3]},
    "oak_planks": {},
}


class DecidedBy(unittest.TestCase):
    def test_a_state_is_read_from_the_fewest_properties_that_decide_it(self):
        rows = [
            ({"facing": f, "half": h}, {"bit": h == "top"})
            for f in ("north", "south")
            for h in ("top", "bottom")
        ]
        rule = registry.decided_by(rows, ["facing", "half"], "bit")
        self.assertEqual(rule, registry.StateRule(("half",), (("bottom", False), ("top", True))))

    def test_a_state_every_row_shares_is_a_constant(self):
        rows = [({"facing": f}, {"bit": True}) for f in ("north", "south")]
        self.assertEqual(
            registry.decided_by(rows, ["facing"], "bit"), registry.StateRule((), (("", True),))
        )

    def test_a_state_some_row_lacks_is_decided_by_nothing(self):
        rows = [({"facing": "north"}, {"bit": True}), ({"facing": "south"}, {})]
        self.assertIsNone(registry.decided_by(rows, ["facing"], "bit"))

    def test_a_state_no_property_decides_is_decided_by_nothing(self):
        rows = [({"facing": "north"}, {"bit": True}), ({"facing": "north"}, {"bit": False})]
        self.assertIsNone(registry.decided_by(rows, ["facing"], "bit"))


class Family(unittest.TestCase):
    def test_a_property_no_state_reads_is_dropped_but_the_water_layer(self):
        rows = [(p, stair_mapping(p)["state"]) for name, p in JAVA.rows if name == "oak_stairs"]
        rows = [(p, {k: registry.bedrock_value(v) for k, v in s.items()}) for p, s in rows]
        family = registry.family_of("oak_stairs", rows, JAVA)
        self.assertEqual(family.dropped, ("shape",))
        self.assertEqual(
            [(state, rule.reads) for state, rule in family.states],
            [("upside_down_bit", ("half",)), ("weirdo_direction", ("facing",))],
        )
        self.assertEqual(family.java[0], ("facing", ("north", "south")))

    def test_states_that_are_no_product_of_their_values_are_refused(self):
        rows = [
            ({"facing": "north", "half": "top", "shape": "straight", "waterlogged": "false"}, {}),
            (
                {"facing": "south", "half": "bottom", "shape": "straight", "waterlogged": "false"},
                {},
            ),
        ]
        with self.assertRaisesRegex(load.Refused, "no product of their values"):
            registry.family_of("oak_stairs", rows, JAVA)


class VersionFamilies(unittest.TestCase):
    def test_each_pair_the_palette_accepts_is_one_family(self):
        version_map = registry.version_families(JAVA, mappings(), BEDROCK_TABLE)
        self.assertEqual(
            sorted(version_map.families),
            [("oak_planks", "oak_planks"), ("oak_stairs", "oak_stairs")],
        )
        self.assertEqual(version_map.refused, [])

    def test_a_pair_the_palette_does_not_declare_is_refused(self):
        palette = {
            **BEDROCK_TABLE,
            "oak_stairs": {"upside_down_bit": [False, True], "weirdo_direction": [0]},
        }
        version_map = registry.version_families(JAVA, mappings(), palette)
        self.assertEqual(sorted(version_map.families), [("oak_planks", "oak_planks")])
        self.assertEqual(version_map.refused, ["oak_stairs"])

    def test_a_pair_that_leaves_out_a_state_the_palette_declares_is_refused(self):
        palette = {**BEDROCK_TABLE, "oak_stairs": {**BEDROCK_TABLE["oak_stairs"], "lit": [False]}}
        version_map = registry.version_families(JAVA, mappings(), palette)
        self.assertEqual(sorted(version_map.families), [("oak_planks", "oak_planks")])
        self.assertEqual(version_map.refused, ["oak_stairs"])

    def test_a_bedrock_state_some_java_state_lacks_fails_the_run(self):
        entries = mappings()
        bit = entries[0]["state"]["upside_down_bit"]
        entries[0] = {**entries[0], "state": {"upside_down_bit": bit}}
        with self.assertRaisesRegex(
            load.Refused, "a Bedrock state of `oak_stairs` is no function of its Java states"
        ):
            registry.version_families(JAVA, entries, BEDROCK_TABLE)

    def test_geysers_placeholder_maps_nothing(self):
        entries = mappings()
        entries[-1] = {"bedrock_identifier": "minecraft:unknown"}
        with mock.patch.object(registry, "PLACEHOLDER_BLOCKS", frozenset({"oak_planks"})):
            version_map = registry.version_families(JAVA, entries, BEDROCK_TABLE)
        self.assertEqual(sorted(version_map.families), [("oak_stairs", "oak_stairs")])
        self.assertEqual(version_map.refused, [])

    def test_a_block_newly_mapped_to_the_placeholder_fails_the_run(self):
        entries = mappings()
        entries[-1] = {"bedrock_identifier": "minecraft:unknown"}
        with self.assertRaisesRegex(
            load.Refused,
            "the state map maps `oak_planks` to no Bedrock block; add it to PLACEHOLDER_BLOCKS",
        ):
            registry.version_families(JAVA, entries, BEDROCK_TABLE)

    def test_a_map_whose_length_is_not_the_java_states_is_refused(self):
        with self.assertRaisesRegex(load.Refused, "entries for"):
            registry.version_families(JAVA, mappings()[:-1], BEDROCK_TABLE)


SHAPE = {"kind": "missing_state", "reason": "stairs render straight"}


class Sensitivity(unittest.TestCase):
    def maps(self):
        return {"1.0": registry.version_families(JAVA, mappings(), BEDROCK_TABLE)}

    def check(self, dropped_states: dict) -> None:
        registry.check_sensitivity(self.maps(), {"dropped_states": dropped_states})

    def test_every_dropped_state_classified_once_passes(self):
        self.check({"shape": [SHAPE]})
        self.check({"shape": [{**SHAPE, "java": ["oak_stairs"]}]})

    def test_a_dropped_state_without_a_row_is_refused(self):
        with self.assertRaisesRegex(load.Refused, "`shape` on `oak_stairs` is unclassified"):
            self.check({})

    def test_a_row_nothing_drops_is_refused(self):
        with self.assertRaisesRegex(load.Refused, "no state map drops `shape` on `birch_stairs`"):
            self.check({"shape": [SHAPE, {**SHAPE, "java": ["birch_stairs"]}]})

    def test_a_block_classified_twice_is_refused(self):
        with self.assertRaisesRegex(load.Refused, "`shape` classifies a block twice"):
            self.check({"shape": [SHAPE, SHAPE]})

    def test_an_unnamed_row_left_covering_nothing_is_refused(self):
        with self.assertRaisesRegex(load.Refused, "`shape`'s unnamed row covers no block"):
            self.check({"shape": [SHAPE, {**SHAPE, "java": ["oak_stairs"]}]})


class StatesText(unittest.TestCase):
    def test_a_family_two_versions_share_is_one_row_and_refusals_are_per_version(self):
        mapped = registry.version_families(JAVA, mappings(), BEDROCK_TABLE)
        refusing = registry.version_families(
            JAVA, mappings(), {**BEDROCK_TABLE, "oak_stairs": {"upside_down_bit": [False]}}
        )
        text = registry.states_text(
            {"1.0": mapped, "1.1": mapped, "1.2": refusing},
            {"dropped_states": {"shape": [SHAPE]}},
        )
        body = json.loads(text)
        self.assertEqual(body["refused"], {"1.2": ["oak_stairs"]})
        self.assertEqual(
            body["in_water"], ["bubble_column", "kelp", "kelp_plant", "seagrass", "tall_seagrass"]
        )
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


# --- the connection table -----------------------------------------------------------------------


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
        {"from": [0, 0, 0], "to": [16, 8, 16], "faces": {f: {} for f in registry.FACES}},
        {"from": [8, 8, 0], "to": [16, 16, 16], "faces": {f: {} for f in registry.FACES}},
    ]
}


class WholeFaces(unittest.TestCase):
    def test_a_face_two_boxes_cover_between_them_is_whole(self):
        boxes = registry.placed_boxes(
            registry.model_boxes(
                archive(
                    {
                        "assets/minecraft/models/block/stairs.json": STAIR_MODEL,
                    }
                ),
                "block/stairs",
                {},
            ),
            0,
            0,
        )
        self.assertEqual(registry.whole_faces(boxes), {"down", "east"})

    def test_a_turned_model_turns_its_whole_faces(self):
        client = archive({"assets/minecraft/models/block/stairs.json": STAIR_MODEL})
        boxes = registry.model_boxes(client, "block/stairs", {})
        # A quarter turn about y takes east to south; a half turn about x takes down to up.
        self.assertEqual(
            registry.whole_faces(registry.placed_boxes(boxes, 0, 90)), {"down", "south"}
        )
        self.assertEqual(registry.whole_faces(registry.placed_boxes(boxes, 180, 0)), {"up", "east"})

    def test_a_box_that_shows_no_face_there_or_is_turned_by_an_angle_covers_nothing(self):
        client = archive(
            {
                "assets/minecraft/models/block/hollow.json": {
                    "elements": [
                        {"from": [0, 0, 0], "to": [16, 16, 16], "faces": {"up": {}, "down": {}}},
                        {
                            "from": [0, 0, 0],
                            "to": [16, 16, 16],
                            "faces": {"north": {}},
                            "rotation": {"origin": [8, 8, 8], "axis": "y", "angle": 45},
                        },
                    ]
                },
            }
        )
        boxes = registry.placed_boxes(registry.model_boxes(client, "block/hollow", {}), 0, 0)
        self.assertEqual(registry.whole_faces(boxes), {"up", "down"})

    def test_a_square_left_open_by_a_sliver_is_not_covered(self):
        self.assertTrue(registry.covers_square([(0, 8.5, 0, 16), (8.5, 16, 0, 16)]))
        self.assertFalse(registry.covers_square([(0, 8, 0, 16), (8.5, 16, 0, 16)]))
        self.assertTrue(registry.covers_square([(6, 10, 6, 10)], (7, 9, 7, 9)))
        self.assertFalse(registry.covers_square([(6, 10, 6, 10)], (7, 9, 0, 9)))

    def test_a_turn_about_x_comes_before_the_turn_about_y(self):
        # Up turns to north about x, and north to east about y; the other order leaves north.
        self.assertEqual(registry.turned((0, 1, 0), 90, 0), (0, 0, -1))
        self.assertEqual(registry.turned((0, 0, -1), 0, 90), (1, 0, 0))
        self.assertEqual(registry.turned((0, 1, 0), 90, 90), (1, 0, 0))


def box(low, high):
    """A box showing every face."""
    return (low, high, frozenset(registry.FACES))


class CoveredParts(unittest.TestCase):
    def test_a_box_resting_on_the_bottom_covers_the_parts_under_it(self):
        lantern = [box((5, 0, 5), (11, 7, 11)), box((6, 7, 6), (10, 9, 10))]
        self.assertEqual(registry.covered_parts(lantern), {"post"})
        slab = [box((0, 0, 0), (16, 8, 16))]
        self.assertEqual(registry.covered_parts(slab), set(registry.WALL_PARTS))
        # A log of a campfire lying east to west covers the post and the strips it crosses.
        log = [box((0, 0, 6), (16, 4, 10))]
        self.assertEqual(registry.covered_parts(log), {"post", "east", "west"})

    def test_a_box_off_the_bottom_or_without_height_covers_nothing(self):
        self.assertEqual(registry.covered_parts([box((0, 8, 0), (16, 16, 16))]), set())
        self.assertEqual(registry.covered_parts([box((0, 0, 0), (16, 0, 16))]), set())


class StateModels(unittest.TestCase):
    def test_a_variant_is_the_one_whose_key_the_state_holds(self):
        blockstate = {
            "variants": {"type=bottom": {"model": "a"}, "type=top": {"model": "b", "x": 180}}
        }
        self.assertEqual(
            registry.state_models(blockstate, {"type": "top", "waterlogged": "false"}),
            [[{"model": "b", "x": 180}]],
        )

    def test_a_multipart_draws_every_part_whose_condition_holds(self):
        blockstate = {
            "multipart": [
                {"apply": {"model": "post"}},
                {"apply": {"model": "side"}, "when": {"north": "low|tall"}},
                {
                    "apply": {"model": "cap"},
                    "when": {"OR": [{"up": "true"}, {"AND": [{"east": "!none"}]}]},
                },
            ]
        }
        self.assertEqual(
            registry.state_models(blockstate, {"north": "tall", "east": "low", "up": "false"}),
            [[{"model": "post"}, {"model": "side"}, {"model": "cap"}]],
        )
        self.assertEqual(
            registry.state_models(blockstate, {"north": "none", "east": "none", "up": "false"}),
            [[{"model": "post"}]],
        )

    def test_a_state_no_variant_matches_is_refused(self):
        with self.assertRaises(load.Refused):
            registry.state_models({"variants": {"type=bottom": {"model": "a"}}}, {"type": "top"})


class TagMembers(unittest.TestCase):
    def test_a_tag_follows_its_nested_tags_and_reads_each_feature_packs_additions(self):
        server = archive(
            {
                "data/minecraft/tags/blocks/walls.json": {
                    "values": ["minecraft:cobblestone_wall", "#minecraft:extra"]
                },
                "data/minecraft/tags/blocks/extra.json": {
                    "values": [{"id": "minecraft:brick_wall", "required": False}]
                },
                "data/minecraft/datapacks/update_1_21/data/minecraft/tags/blocks/walls.json": {
                    "values": ["minecraft:tuff_wall"]
                },
            }
        )
        self.assertEqual(
            registry.tag_members(server, "walls"), {"cobblestone_wall", "brick_wall", "tuff_wall"}
        )

    def test_a_tag_a_feature_pack_replaces_is_refused(self):
        server = archive(
            {
                "data/minecraft/tags/block/walls.json": {"values": []},
                "data/minecraft/datapacks/x/data/minecraft/tags/block/walls.json": {
                    "replace": True,
                    "values": [],
                },
            }
        )
        with self.assertRaises(load.Refused):
            registry.tag_members(server, "walls")


TAGS = {
    "walls": {"cobblestone_wall"},
    "fences": {"oak_fence", "nether_brick_fence"},
    "wooden_fences": {"oak_fence"},
    "fence_gates": set(),
}
FENCE_STATES = {**registry.KIND_STATES["fence"], "waterlogged": registry.BOOLEAN}


class BlockKind(unittest.TestCase):
    def test_a_pane_is_a_block_with_a_panes_states_that_is_no_fence(self):
        self.assertEqual(registry.block_kind("glass_pane", FENCE_STATES, TAGS), "pane")
        self.assertEqual(registry.block_kind("oak_fence", FENCE_STATES, TAGS), "wooden_fence")
        self.assertEqual(registry.block_kind("nether_brick_fence", FENCE_STATES, TAGS), "fence")
        self.assertIsNone(registry.block_kind("stone", {}, TAGS))

    def test_a_chest_is_a_block_whose_type_pairs_it(self):
        chest = {**registry.KIND_STATES["chest"], "waterlogged": registry.BOOLEAN}
        self.assertEqual(registry.block_kind("trapped_chest", chest, TAGS), "chest")
        ender = {"facing": chest["facing"], "waterlogged": registry.BOOLEAN}
        self.assertIsNone(registry.block_kind("ender_chest", ender, TAGS))
        slab = {"type": ["top", "bottom", "double"], "waterlogged": registry.BOOLEAN}
        self.assertIsNone(registry.block_kind("oak_slab", slab, TAGS))

    def test_a_tagged_block_without_its_kinds_states_is_refused(self):
        with self.assertRaises(load.Refused):
            registry.block_kind("cobblestone_wall", FENCE_STATES, TAGS)


FULL_MODEL = {
    "elements": [{"from": [0, 0, 0], "to": [16, 16, 16], "faces": {f: {} for f in registry.FACES}}]
}
LANTERN_MODEL = {"elements": [{"from": [5, 0, 5], "to": [11, 7, 11], "faces": {"up": {}}}]}


def connection_jars() -> tuple[zipfile.ZipFile, zipfile.ZipFile]:
    """A server declaring the tags the rules read, and a client drawing a few blocks."""
    tags = {
        "walls": ["minecraft:cobblestone_wall"],
        "fences": [],
        "wooden_fences": [],
        "fence_gates": ["minecraft:oak_fence_gate"],
        "wall_post_override": ["minecraft:torch"],
        "leaves": ["minecraft:oak_leaves"],
        "shulker_boxes": [],
    }
    server = archive(
        {f"data/minecraft/tags/block/{tag}.json": {"values": v} for tag, v in tags.items()}
    )
    full = {"variants": {"": {"model": "block/full"}}}
    client = archive(
        {
            "assets/minecraft/models/block/full.json": FULL_MODEL,
            "assets/minecraft/models/block/lantern.json": LANTERN_MODEL,
            "assets/minecraft/blockstates/stone.json": full,
            "assets/minecraft/blockstates/oak_leaves.json": full,
            "assets/minecraft/blockstates/cobblestone_wall.json": full,
            "assets/minecraft/blockstates/torch.json": {"variants": {"": {"model": "block/air"}}},
            "assets/minecraft/models/block/air.json": {},
            "assets/minecraft/blockstates/lantern.json": {
                "variants": {
                    "hanging=false": {"model": "block/lantern"},
                    "hanging=true": {"model": "block/air"},
                }
            },
            "assets/minecraft/blockstates/oak_fence_gate.json": {
                "variants": {"": {"model": "block/air"}}
            },
        }
    )
    return server, client


def report_block(properties: dict[str, list[str]], default: dict[str, str]) -> dict:
    """A `blocks.json` entry with every state of `properties`, `default` the default one."""
    keys = sorted(properties)
    states = []
    for values in itertools.product(*(properties[k] for k in keys)):
        state = dict(zip(keys, values))
        states.append({"properties": state, **({"default": True} if state == default else {})})
    return {"properties": properties, "states": states} if keys else {"states": [{"default": True}]}


class ConnectionTable(unittest.TestCase):
    def test_each_block_is_read_with_its_kind_tags_faces_and_covered_parts(self):
        server, client = connection_jars()
        gate_states = {
            "facing": ["north", "south", "west", "east"],
            "in_wall": registry.BOOLEAN,
            "open": registry.BOOLEAN,
            "powered": registry.BOOLEAN,
        }
        report = {
            "minecraft:stone": report_block({}, {}),
            "minecraft:oak_leaves": report_block({}, {}),
            "minecraft:torch": report_block({}, {}),
            "minecraft:lantern": report_block({"hanging": registry.BOOLEAN}, {"hanging": "false"}),
            "minecraft:cobblestone_wall": report_block(
                {**registry.KIND_STATES["wall"], "waterlogged": registry.BOOLEAN},
                {
                    "east": "none",
                    "north": "none",
                    "south": "none",
                    "west": "none",
                    "up": "true",
                    "waterlogged": "false",
                },
            ),
            "minecraft:oak_fence_gate": report_block(
                gate_states,
                {"facing": "north", "in_wall": "false", "open": "false", "powered": "false"},
            ),
        }
        every_face = registry.Lookup((), (("", tuple(registry.FACES)),))
        every_part = registry.Lookup((), (("", tuple(registry.WALL_PARTS)),))
        self.assertEqual(
            registry.connection_table(report, server, client),
            {
                "stone": registry.Joins(None, False, False, sturdy=every_face, covers=every_part),
                "oak_leaves": registry.Joins(
                    None, True, False, sturdy=every_face, covers=every_part
                ),
                "torch": registry.Joins(None, False, True, sturdy=None),
                "lantern": registry.Joins(
                    None,
                    False,
                    False,
                    sturdy=None,
                    covers=registry.Lookup(("hanging",), (("false", ("post",)), ("true", ()))),
                    defaults=(("hanging", "false"),),
                ),
                # A connecting block's faces are its own, and what it covers is its states'.
                "cobblestone_wall": registry.Joins("wall", False, False, sturdy=every_face),
                "oak_fence_gate": registry.Joins(
                    "fence_gate",
                    False,
                    False,
                    sturdy=None,
                    defaults=(("facing", "north"), ("open", "false")),
                ),
            },
        )

    def test_a_block_the_client_ships_no_blockstate_for_is_refused(self):
        server, client = connection_jars()
        with self.assertRaises(load.Refused):
            registry.connection_table({"minecraft:dirt": report_block({}, {})}, server, client)


class ConnectionsText(unittest.TestCase):
    def test_a_block_read_alike_at_two_versions_is_one_row_with_both(self):
        wall = registry.Joins("wall", False, False, None)
        slab = registry.Joins(
            None,
            False,
            False,
            sturdy=registry.Lookup(("type",), (("bottom", ("down",)), ("top", ("up",)))),
            covers=registry.Lookup(("type",), (("bottom", ("post",)), ("top", ()))),
            defaults=(("type", "bottom"),),
        )
        body = json.loads(
            registry.connections_text(
                {
                    "1.0": {"cobblestone_wall": wall, "oak_slab": slab},
                    "1.1": {"cobblestone_wall": wall, "tuff_wall": wall},
                }
            )
        )
        self.assertEqual(
            body["rows"],
            [
                {"mc_versions": ["1.0", "1.1"], "ids": ["cobblestone_wall"], "kind": "wall"},
                {
                    "mc_versions": ["1.0"],
                    "ids": ["oak_slab"],
                    "defaults": {"type": "bottom"},
                    "sturdy": {"reads": ["type"], "map": {"bottom": ["down"], "top": ["up"]}},
                    "covers": {"reads": ["type"], "map": {"bottom": ["post"], "top": []}},
                },
                {"mc_versions": ["1.1"], "ids": ["tuff_wall"], "kind": "wall"},
            ],
        )


if __name__ == "__main__":
    unittest.main()
