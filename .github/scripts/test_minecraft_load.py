"""Tests for minecraft_load.py, run as `python3 -m unittest discover -s .github/scripts`."""

from __future__ import annotations

import contextlib
import copy
import gzip
import io
import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import minecraft_load as load


def structure(*blocks: tuple[tuple[int, int, int], dict], data_version: int = 4189) -> dict:
    """A structure holding `blocks`, each a position and a palette entry, one entry per block."""
    return {
        "DataVersion": data_version,
        "size": [2, 1, 1],
        "palette": [copy.deepcopy(entry) for _, entry in blocks],
        "blocks": [{"pos": list(pos), "state": i} for i, (pos, _) in enumerate(blocks)],
        "entities": [],
    }


STAIR = {"Name": "minecraft:oak_stairs", "Properties": {"facing": "north", "half": "top"}}
PLANKS = {"Name": "minecraft:oak_planks"}
STAIR_DEFAULTS = {"facing": "north", "half": "bottom", "shape": "straight", "waterlogged": "false"}


def written(value: dict) -> dict:
    """`value` as `read_nbt` gives it back after `write_nbt`, its integers typed by their tag."""
    return load.read_nbt(load.write_nbt(value))


class Nbt(unittest.TestCase):
    def test_a_structure_reads_back_as_written(self) -> None:
        value = structure(((0, 0, 0), STAIR), ((1, 0, 0), PLANKS))
        self.assertEqual(written(value), value)

    def test_a_file_that_ends_inside_a_tag_is_refused(self) -> None:
        raw = gzip.decompress(load.write_nbt(structure(((0, 0, 0), PLANKS))))
        with self.assertRaisesRegex(ValueError, "NBT ends inside a tag"):
            load.read_nbt(gzip.compress(raw[:-3]))

    def test_a_root_that_is_not_a_compound_is_refused(self) -> None:
        with self.assertRaisesRegex(ValueError, "the root tag is not a compound"):
            load.read_nbt(gzip.compress(bytes([load.TAG_STRING, 0, 0, 0, 0])))


class StrictLayout(unittest.TestCase):
    def test_the_layout_cairn_writes_is_accepted(self) -> None:
        load.strict_layout(written(structure(((0, 0, 0), STAIR), ((1, 0, 0), PLANKS))))

    def test_a_key_the_game_does_not_read_or_a_mistyped_position_is_refused(self) -> None:
        def misspelt_properties(s: dict) -> None:
            s["palette"][0]["properties"] = s["palette"][0].pop("Properties")

        def misspelt_blocks(s: dict) -> None:
            s["Blocks"] = s.pop("blocks")

        def misspelt_block_entity(s: dict) -> None:
            s["blocks"][0]["NBT"] = {"id": "minecraft:sign"}

        def misspelt_entities(s: dict) -> None:
            s["Entities"] = s.pop("entities")

        def repeated_position(s: dict) -> None:
            s["blocks"][1]["pos"] = [0, 0, 0]

        def short_position(s: dict) -> None:
            s["blocks"][0]["pos"] = [load.Short(0)] * 3

        def state_past_the_palette(s: dict) -> None:
            s["blocks"][0]["state"] = 2

        for change, problem in [
            (
                misspelt_properties,
                "a palette entry has `properties`, which the game does not read",
            ),
            (misspelt_blocks, "the root has `Blocks`, which the game does not read"),
            (misspelt_block_entity, "a block has `NBT`, which the game does not read"),
            (misspelt_entities, "the root has `Entities`, which the game does not read"),
            (repeated_position, "two blocks are at (0, 0, 0)"),
            (short_position, "a block's `pos` is not three `Int`s"),
            (
                state_past_the_palette,
                "the block at (0, 0, 0) has no `Int` `state` inside its palette",
            ),
        ]:
            with self.subTest(change.__name__):
                value = structure(((0, 0, 0), STAIR), ((1, 0, 0), PLANKS))
                change(value)
                with self.assertRaises(load.Malformed) as refusal:
                    load.strict_layout(written(value))
                self.assertEqual(str(refusal.exception), problem)


class Differences(unittest.TestCase):
    def differences(self, loaded: dict) -> list[str]:
        value = structure(((0, 0, 0), STAIR), ((1, 0, 0), PLANKS))
        return load.differences(value, loaded)

    def test_the_same_blocks_in_another_palette_order_are_no_difference(self) -> None:
        self.assertEqual(self.differences(structure(((1, 0, 0), PLANKS), ((0, 0, 0), STAIR))), [])

    def test_a_default_property_the_game_saves_is_no_difference(self) -> None:
        stair = {**STAIR, "Properties": {**STAIR["Properties"], "waterlogged": "false"}}
        self.assertEqual(self.differences(structure(((0, 0, 0), stair), ((1, 0, 0), PLANKS))), [])

    def test_a_block_read_as_another_id_is_named(self) -> None:
        loaded = structure(((0, 0, 0), STAIR), ((1, 0, 0), {"Name": "minecraft:air"}))
        self.assertEqual(
            self.differences(loaded), ["(1, 0, 0): minecraft:oak_planks was read as minecraft:air"]
        )

    def test_a_changed_property_is_read_as_and_a_missing_one_is_dropped(self) -> None:
        stair = {"Name": "minecraft:oak_stairs", "Properties": {"facing": "south"}}
        self.assertEqual(
            self.differences(structure(((0, 0, 0), stair), ((1, 0, 0), PLANKS))),
            [
                "(0, 0, 0): minecraft:oak_stairs[facing=north] read as facing=south",
                "(0, 0, 0): minecraft:oak_stairs[half=top] dropped",
            ],
        )

    def test_a_new_data_version_size_entity_count_and_moved_block_are_named(self) -> None:
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

    def test_a_block_count_the_game_reads_differently_is_named(self) -> None:
        self.assertEqual(
            self.differences(structure(((0, 0, 0), STAIR))),
            ["2 blocks were read as 1", "(1, 0, 0): minecraft:oak_planks was dropped"],
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
        versions = ("1.9", "1.20.4", "1.20.10", "1.21", "26.1")
        folders = [load.Target(v, 0).structure_folder() for v in versions]
        self.assertEqual(
            folders, ["structures", "structures", "structures", "structure", "structure"]
        )


def completed(stdout: str = "", stderr: str = "") -> subprocess.CompletedProcess:
    """A run that exited 0 and printed `stdout` and `stderr`."""
    return subprocess.CompletedProcess([], 0, stdout, stderr)


class Guards(unittest.TestCase):
    """`check_target` offline: the server jar, Java, `cairn` and the data generator are faked."""

    TARGET = load.Target("1.21.4", 4189)

    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        self.examples = self.tmp / "examples"
        self.examples.mkdir()
        for name in ("a", "b"):
            (self.examples / f"{name}.crn").write_text("")
        self.written = {"a": structure(((0, 0, 0), STAIR)), "b": structure(((0, 0, 0), PLANKS))}
        self.patch("EXAMPLES", self.examples)
        self.patch("server_jar", lambda version, cache: (self.tmp / "server.jar", 21))
        self.patch("java_major", lambda java: 21)
        self.patch("compile_examples", self.compile_examples)

    def patch(self, name: str, value) -> None:
        patcher = mock.patch.object(load, name, value)
        patcher.start()
        self.addCleanup(patcher.stop)

    def compile_examples(self, cairn, target, into: Path, locks) -> list[Path]:
        files = []
        for name, value in self.written.items():
            (into / name).mkdir(parents=True)
            files.append(into / name / f"{name}.nbt")
            files[-1].write_bytes(load.write_nbt(value))
        return files

    def generator(self, loads: bool = True, skip: str = ""):
        """A data generator whose `--server` run writes each input to `out/`, loaded or not."""

        def run(java, jar, cwd, log, mode: str, *args: str) -> None:
            options = dict(zip(args[::2], args[1::2]))
            if mode != "--server":
                return
            source = Path(options["--input"]).parent / "in"
            for path in source.rglob("*.nbt"):
                if path.stem == skip:
                    continue
                value = load.read_nbt(path.read_bytes())
                for entry in value["palette"]:
                    if loads and entry["Name"] == "minecraft:oak_stairs":
                        entry["Properties"] = {**STAIR_DEFAULTS, **entry.get("Properties", {})}
                reloaded = Path(options["--output"]) / path.relative_to(source)
                reloaded.parent.mkdir(parents=True, exist_ok=True)
                reloaded.write_bytes(load.write_nbt(value))

        return run

    def check(self) -> list[str]:
        with contextlib.redirect_stdout(io.StringIO()):
            return load.check_target("cairn", "java", self.TARGET, self.tmp, self.tmp / "work")

    def test_a_loading_generator_with_matching_copies_finds_nothing(self) -> None:
        self.patch("data_generator", self.generator())
        self.assertEqual(self.check(), [])

    def test_a_generator_that_copies_without_loading_is_refused(self) -> None:
        self.patch("data_generator", self.generator(loads=False))
        with self.assertRaises(load.Refused) as refusal:
            self.check()
        self.assertEqual(
            str(refusal.exception),
            "the 1.21.4 data generator converted the structures without loading them; it reads "
            "structures from a folder other than `structure`",
        )

    def test_a_generator_that_writes_no_sentinel_is_not_blamed_on_the_folder(self) -> None:
        self.patch("data_generator", self.generator(skip="sentinel"))
        with self.assertRaises(load.Refused) as refusal:
            self.check()
        self.assertEqual(
            str(refusal.exception),
            "the 1.21.4 data generator wrote no copy of the sentinel structure "
            f"(logs in {self.tmp / 'work' / '1.21.4'})",
        )

    def test_a_structure_the_game_wrote_no_copy_of_is_named(self) -> None:
        self.patch("data_generator", self.generator(skip="b"))
        self.assertEqual(self.check(), ["Java 1.21.4 b/b.nbt: the game wrote no copy of it"])

    def test_a_malformed_written_structure_is_named(self) -> None:
        self.written["b"]["blocks"].append({"pos": [0, 0, 0], "state": 0})
        self.patch("data_generator", self.generator())
        self.assertEqual(
            self.check(),
            ["Java 1.21.4 b/b.nbt: the written file is malformed: two blocks are at (0, 0, 0)"],
        )


class Refusals(unittest.TestCase):
    TARGET = load.Target("1.21.4", 4189)

    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        self.examples = self.tmp / "examples"
        self.examples.mkdir()
        patcher = mock.patch.object(load, "EXAMPLES", self.examples)
        patcher.start()
        self.addCleanup(patcher.stop)

    def compile(self, run: subprocess.CompletedProcess) -> str:
        with mock.patch.object(load, "run_tool", lambda command, cwd=None: run):
            with self.assertRaises(load.Refused) as refusal:
                load.compile_examples("cairn", self.TARGET, self.tmp / "out", self.tmp / "locks")
        return str(refusal.exception)

    def test_no_example_is_refused(self) -> None:
        self.assertEqual(self.compile(completed()), f"{self.examples} holds no `.crn` example")

    def test_an_example_that_warns_or_writes_nothing_is_refused(self) -> None:
        source = self.examples / "a.crn"
        source.write_text("")
        self.assertEqual(
            self.compile(completed(stderr="warning[W_DEFERRED_MEMBER]: …\n")),
            f"{source} does not compile clean for Java 1.21.4 (exit 0):\n"
            "warning[W_DEFERRED_MEMBER]: …",
        )
        self.assertEqual(self.compile(completed()), f"{source} wrote no .nbt for Java 1.21.4")

    def test_a_generator_run_that_logs_an_exception_is_refused_whatever_its_exit(self) -> None:
        for run in (
            completed(stdout="[12:00:00] [ServerMain/ERROR]: Failed to convert a.nbt\n"),
            completed(stderr='Exception in thread "ServerMain" java.lang.RuntimeException\n'),
        ):
            with self.subTest(run.stdout or run.stderr):
                log = self.tmp / "server.log"
                with mock.patch.object(load, "run_tool", lambda command, cwd=None: run):
                    with self.assertRaises(load.Refused) as refusal:
                        load.data_generator("java", self.tmp / "s.jar", self.tmp, log, "--server")
                self.assertEqual(
                    str(refusal.exception),
                    f"the data generator's --server run failed (exit 0, log in {log}):\n"
                    f"{(run.stdout + run.stderr).strip()}",
                )
                self.assertEqual(log.read_text(encoding="utf-8"), run.stdout + run.stderr)


class Main(unittest.TestCase):
    def main(self, *argv: str, versions: list[dict] | None = None, found=()) -> tuple[int, str]:
        table = Path(tempfile.mkdtemp()) / "data_versions.json"
        self.addCleanup(shutil.rmtree, table.parent)
        if versions is None:
            versions = [{"mc_version": "1.21.4", "data_version": 4189}]
        table.write_text(json.dumps({"versions": versions}))
        stderr = io.StringIO()
        with (
            mock.patch.object(load, "DATA_VERSIONS", table),
            mock.patch.object(load, "check_target", lambda *args: list(found)),
            contextlib.redirect_stderr(stderr),
        ):
            code = load.main(["--cairn", "cairn", "--work", str(table.parent), *argv])
        return code, stderr.getvalue()

    def test_a_difference_fails_the_run_and_none_passes_it(self) -> None:
        line = "Java 1.21.4 a/a.nbt: x"
        self.assertEqual(self.main(found=[line]), (1, f"{line}\n"))
        self.assertEqual(self.main(), (0, ""))

    def test_an_unknown_target_is_a_usage_error(self) -> None:
        self.assertEqual(self.main("--target", "1.8"), (2, "not a shipped Java target: 1.8\n"))

    def test_a_table_with_no_targetable_version_is_refused(self) -> None:
        rows = [{"mc_version": "1.20.3", "data_version": 3698, "targetable": False}]
        code, stderr = self.main(versions=rows)
        self.assertEqual(code, 1)
        self.assertRegex(stderr, r"^error: .*data_versions\.json has no targetable Java version\n$")


if __name__ == "__main__":
    unittest.main()
