"""Tests for extract_registry.py, run as `python3 -m unittest discover -s .github/scripts`."""

from __future__ import annotations

import contextlib
import hashlib
import io
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


if __name__ == "__main__":
    unittest.main()
