#!/usr/bin/env python3
"""Load every example's `.nbt` with Minecraft Java's own structure loader, at each shipped target.

Each `examples/*.crn` is compiled for every targetable row of the built-in Java pack, and that
version's server jar reads each `.nbt` back through its data generator: `--dev` writes SNBT, and
`--server` runs the DataFixer from the file's `DataVersion` and loads the template against the
block registry. An unknown block loads as air, an unknown property is dropped and a value outside
its domain reverts to the default. Unlike a running world, the loader does not filter blocks by
enabled features, so a block behind an experiment loads here and not in the game.

Standard library only: it runs on a bare `ubuntu-latest` runner.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import http.client
import io
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import urllib.request
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
EXAMPLES = ROOT / "examples"
DATA_VERSIONS = ROOT / "crates/cairn-lang-formats/registry-data/java/data_versions.json"
MANIFEST_URL = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"

SHOWN_PER_FILE = 5

# The data generator exits 0 even when a conversion throws, so its log is the only sign.
GENERATOR_FAILURES = ('Exception in thread "ServerMain"', "/ERROR]:")

TAG_END, TAG_BYTE, TAG_SHORT, TAG_INT, TAG_LONG = 0, 1, 2, 3, 4
TAG_FLOAT, TAG_DOUBLE, TAG_BYTE_ARRAY, TAG_STRING = 5, 6, 7, 8
TAG_LIST, TAG_COMPOUND, TAG_INT_ARRAY, TAG_LONG_ARRAY = 9, 10, 11, 12


class Refused(Exception):
    """A step the check depends on did not complete."""


class Malformed(ValueError):
    """A written structure laid out other than the way the game reads one."""


def shown(path: Path) -> str:
    """`path` relative to the repository when it is inside it."""
    return str(path.relative_to(ROOT)) if path.is_relative_to(ROOT) else str(path)


# --- NBT ------------------------------------------------------------------------------------


class Byte(int):
    """An NBT `Byte`."""


class Short(int):
    """An NBT `Short`."""


class Int(int):
    """An NBT `Int`."""


class Long(int):
    """An NBT `Long`."""


INTEGERS = {
    TAG_BYTE: (Byte, ">b"),
    TAG_SHORT: (Short, ">h"),
    TAG_INT: (Int, ">i"),
    TAG_LONG: (Long, ">q"),
}


def read_nbt(raw: bytes) -> dict:
    """The root compound of a gzip-compressed, big-endian NBT file, integers typed by their tag."""
    data = gzip.decompress(raw)
    pos = 0

    def take(n: int) -> bytes:
        nonlocal pos
        if pos + n > len(data):
            raise ValueError("NBT ends inside a tag")
        chunk = data[pos : pos + n]
        pos += n
        return chunk

    def unpack(fmt: str) -> int | float:
        return struct.unpack(fmt, take(struct.calcsize(fmt)))[0]

    def string() -> str:
        return take(int(unpack(">H"))).decode("utf-8")

    def payload(tag: int):
        if tag in INTEGERS:
            kind, fmt = INTEGERS[tag]
            return kind(unpack(fmt))
        if tag in (TAG_FLOAT, TAG_DOUBLE):
            return unpack(">f" if tag == TAG_FLOAT else ">d")
        if tag == TAG_BYTE_ARRAY:
            return list(take(int(unpack(">i"))))
        if tag == TAG_STRING:
            return string()
        if tag == TAG_LIST:
            item = take(1)[0]
            return [payload(item) for _ in range(int(unpack(">i")))]
        if tag == TAG_COMPOUND:
            out = {}
            while (child := take(1)[0]) != TAG_END:
                name = string()
                out[name] = payload(child)
            return out
        if tag in (TAG_INT_ARRAY, TAG_LONG_ARRAY):
            n = int(unpack(">i"))
            width = 4 if tag == TAG_INT_ARRAY else 8
            return list(struct.unpack(f">{n}{'i' if width == 4 else 'q'}", take(width * n)))
        raise ValueError(f"unknown NBT tag {tag}")

    if take(1)[0] != TAG_COMPOUND:
        raise ValueError("the root tag is not a compound")
    string()
    return payload(TAG_COMPOUND)


def write_nbt(root: dict) -> bytes:
    """`root` as a gzip-compressed NBT file: integers, strings, lists and compounds only.

    A plain `int` is written as an `Int`, and `Byte`, `Short` and `Long` as their own tags.
    """
    out = io.BytesIO()

    def tag_of(value) -> int:
        for tag, (kind, _) in INTEGERS.items():
            if type(value) is kind:
                return tag
        if isinstance(value, bool) or not isinstance(value, (int, str, list, dict)):
            raise TypeError(f"no NBT tag for {value!r}")
        return {int: TAG_INT, str: TAG_STRING, list: TAG_LIST, dict: TAG_COMPOUND}[type(value)]

    def string(text: str) -> None:
        encoded = text.encode("utf-8")
        out.write(struct.pack(">H", len(encoded)) + encoded)

    def payload(value) -> None:
        tag = tag_of(value)
        if tag in INTEGERS:
            out.write(struct.pack(INTEGERS[tag][1], value))
        elif tag == TAG_STRING:
            string(value)
        elif tag == TAG_LIST:
            item = tag_of(value[0]) if value else TAG_END
            out.write(bytes([item]) + struct.pack(">i", len(value)))
            for element in value:
                payload(element)
        else:
            for name, child in value.items():
                out.write(bytes([tag_of(child)]))
                string(name)
                payload(child)
            out.write(bytes([TAG_END]))

    out.write(bytes([TAG_COMPOUND]))
    string("")
    payload(root)
    return gzip.compress(out.getvalue(), mtime=0)


# --- comparing a written structure with the game's copy ------------------------------------

ROOT_KEYS = {"DataVersion", "size", "palette", "palettes", "blocks", "entities"}
PALETTE_KEYS = {"Name", "Properties"}
BLOCK_KEYS = {"pos", "state", "nbt"}
ENTITY_KEYS = {"pos", "blockPos", "nbt"}


def strict_layout(structure: dict) -> None:
    """Raise `Malformed` unless `structure` uses only the keys and tags the game reads.

    The game skips a key it does not read and reads a mistyped `pos` or `size` as zeros, so a
    lenient comparison would agree with a file the game reads as empty.
    """

    def require(ok: bool, problem: str) -> None:
        if not ok:
            raise Malformed(problem)

    def compound(value, allowed: set[str], what: str) -> dict:
        require(isinstance(value, dict), f"{what} is not a compound")
        extra = sorted(value.keys() - allowed)
        named = ", ".join(f"`{key}`" for key in extra)
        require(not extra, f"{what} has {named}, which the game does not read")
        return value

    def ints(value, what: str) -> tuple[int, int, int]:
        three = isinstance(value, list) and len(value) == 3
        require(three and all(type(v) is Int for v in value), f"{what} is not three `Int`s")
        return tuple(value)

    compound(structure, ROOT_KEYS, "the root")
    missing = sorted({"DataVersion", "size", "blocks", "entities"} - structure.keys())
    require(not missing, f"the root has no {', '.join(f'`{key}`' for key in missing)}")
    one = ("palette" in structure) != ("palettes" in structure)
    require(one, "the root needs exactly one of `palette` and `palettes`")
    require(type(structure["DataVersion"]) is Int, "`DataVersion` is not an `Int`")
    ints(structure["size"], "`size`")
    palettes = [structure["palette"]] if "palette" in structure else structure["palettes"]
    require(isinstance(palettes, list) and bool(palettes), "`palettes` holds no palette")
    for palette in palettes:
        require(isinstance(palette, list), "a palette is not a list")
        for entry in palette:
            compound(entry, PALETTE_KEYS, "a palette entry")
            require(isinstance(entry.get("Name"), str), "a palette entry has no `Name`")
            properties = entry.get("Properties", {})
            textual = isinstance(properties, dict) and all(
                isinstance(v, str) for v in properties.values()
            )
            require(textual, f"{entry['Name']}'s `Properties` are not all strings")
    require(isinstance(structure["blocks"], list), "`blocks` is not a list")
    seen = set()
    for block in structure["blocks"]:
        compound(block, BLOCK_KEYS, "a block")
        pos = ints(block.get("pos"), "a block's `pos`")
        state = block.get("state")
        in_palette = type(state) is Int and all(0 <= state < len(p) for p in palettes)
        require(in_palette, f"the block at {pos} has no `Int` `state` inside its palette")
        require(pos not in seen, f"two blocks are at {pos}")
        seen.add(pos)
    require(isinstance(structure["entities"], list), "`entities` is not a list")
    for entity in structure["entities"]:
        compound(entity, ENTITY_KEYS, "an entity")


def block_states(structure: dict) -> dict[tuple[int, int, int], tuple[str, dict]]:
    """Each block of a structure by position, as its id and properties."""
    palette = structure["palette"] if "palette" in structure else structure["palettes"][0]
    states = {}
    for block in structure.get("blocks", []):
        entry = palette[block["state"]]
        states[tuple(block["pos"])] = (entry["Name"], entry.get("Properties", {}))
    return states


def differences(written: dict, loaded: dict) -> list[str]:
    """What the game read differently from what was written, one line per difference.

    The game saves every property of a state, so a property the written file leaves out is not a
    difference; one it writes must come back with the value it wrote.
    """
    found = []
    for key in ("DataVersion", "size"):
        if written.get(key) != loaded.get(key):
            found.append(f"{key} {written.get(key)} was read as {loaded.get(key)}")
    for key in ("entities", "blocks"):
        ours, theirs = len(written.get(key, [])), len(loaded.get(key, []))
        if ours != theirs:
            found.append(f"{ours} {key} were read as {theirs}")
    ours, theirs = block_states(written), block_states(loaded)
    for pos in sorted(ours.keys() | theirs.keys()):
        if pos not in theirs:
            found.append(f"{pos}: {ours[pos][0]} was dropped")
            continue
        if pos not in ours:
            found.append(f"{pos}: {theirs[pos][0]} appeared")
            continue
        (name, properties), (read_name, read_properties) = ours[pos], theirs[pos]
        if name != read_name:
            found.append(f"{pos}: {name} was read as {read_name}")
            continue
        for key, value in properties.items():
            if read_properties.get(key) != value:
                read = read_properties.get(key)
                change = "dropped" if read is None else f"read as {key}={read}"
                found.append(f"{pos}: {name}[{key}={value}] {change}")
    return found


def compare(path: Path, copy: Path) -> list[str]:
    """What the game read differently from the structure at `path`, given its copy at `copy`."""
    if not copy.exists():
        return ["the game wrote no copy of it"]
    try:
        written = read_nbt(path.read_bytes())
        strict_layout(written)
    except Malformed as problem:
        return [f"the written file is malformed: {problem}"]
    except (OSError, EOFError, ValueError) as problem:
        return [f"the written file does not read as NBT: {problem}"]
    try:
        loaded = read_nbt(copy.read_bytes())
    except (OSError, EOFError, ValueError) as problem:
        return [f"the game's copy does not read as NBT: {problem}"]
    return differences(written, loaded)


# --- targets and server jars -----------------------------------------------------------------


@dataclass(frozen=True)
class Target:
    """A Java version the built-in pack can build for, and the `DataVersion` it writes."""

    mc_version: str
    data_version: int

    def structure_folder(self) -> str:
        """The datapack folder this version reads structures from, which 1.21 made singular."""
        return "structure" if version_key(self.mc_version) >= (1, 21) else "structures"


def version_key(version: str) -> tuple[int, ...]:
    """`version` as numbers, compared component by component."""
    return tuple(int(part) for part in version.split("."))


def shipped_targets(table: dict) -> list[Target]:
    """The targetable rows of a `data_versions.json`, in table order."""
    return [
        Target(row["mc_version"], row["data_version"])
        for row in table["versions"]
        if row.get("targetable", True)
    ]


def fetch(url: str, timeout: int) -> bytes:
    """The body at `url`."""
    try:
        with urllib.request.urlopen(url, timeout=timeout) as response:
            return response.read()
    except (OSError, http.client.HTTPException) as problem:
        raise Refused(f"cannot download {url}: {problem}") from None


def fetch_json(url: str) -> dict:
    """The JSON document at `url`."""
    try:
        return json.loads(fetch(url, 60))
    except ValueError as problem:
        raise Refused(f"{url} is not JSON: {problem}") from None


def write_atomically(path: Path, data: bytes) -> None:
    """`data` at `path`, which an interrupted run never leaves half written."""
    partial = path.with_name(f"{path.name}.partial")
    partial.write_bytes(data)
    os.replace(partial, path)


def server_jar(version: str, cache: Path) -> tuple[Path, int]:
    """The vanilla server jar for `version` in `cache`, and the Java major version it needs."""
    cache.mkdir(parents=True, exist_ok=True)
    meta_path = cache / f"{version}.json"
    try:
        meta = json.loads(meta_path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        manifest = fetch_json(MANIFEST_URL)
        urls = [v["url"] for v in manifest["versions"] if v["id"] == version]
        if not urls:
            raise Refused(f"Mojang's version manifest has no {version}") from None
        meta = fetch_json(urls[0])
        write_atomically(meta_path, json.dumps(meta).encode("utf-8"))
    download = meta["downloads"]["server"]
    jar = cache / f"server-{version}.jar"
    if not jar.exists() or hashlib.sha1(jar.read_bytes()).hexdigest() != download["sha1"]:
        body = fetch(download["url"], 300)
        if hashlib.sha1(body).hexdigest() != download["sha1"]:
            raise Refused(f"the {version} server jar does not match the SHA-1 Mojang publishes")
        write_atomically(jar, body)
    return jar, int(meta["javaVersion"]["majorVersion"])


def run_tool(command: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess:
    """`command` run to completion, with its output captured as UTF-8."""
    try:
        return subprocess.run(
            command, cwd=cwd, capture_output=True, encoding="utf-8", errors="replace"
        )
    except OSError as problem:
        raise Refused(f"cannot run `{command[0]}`: {problem}") from None


def java_major(java: str) -> int:
    """The major version of the `java` executable."""
    shown_settings = run_tool([java, "-XshowSettings:properties", "-version"]).stderr
    for line in shown_settings.splitlines():
        if line.strip().startswith("java.specification.version"):
            return int(line.split("=")[1].strip().split(".")[-1])
    raise Refused(f"cannot tell which Java `{java}` is")


# --- the check -------------------------------------------------------------------------------


def compile_examples(cairn: str, target: Target, into: Path, locks: Path) -> list[Path]:
    """The `.nbt` files of each example compiled for `target`, in a folder per example in `into`."""
    sources = sorted(EXAMPLES.glob("*.crn"))
    if not sources:
        raise Refused(f"{shown(EXAMPLES)} holds no `.crn` example")
    locks.mkdir(parents=True, exist_ok=True)
    written = []
    for source in sources:
        out = into / source.stem
        run = run_tool(
            [
                cairn, "compile", "--edition", "java", "--target", target.mc_version,
                "--out", str(out), "--lock", str(locks / f"{source.stem}.lock"), str(source),
            ]
        )
        if run.returncode != 0 or run.stderr.strip():
            raise Refused(
                f"{shown(source)} does not compile clean for Java {target.mc_version} "
                f"(exit {run.returncode}):\n{run.stderr.strip()}"
            )
        files = sorted(out.glob("*.nbt"))
        if not files:
            raise Refused(f"{shown(source)} wrote no .nbt for Java {target.mc_version}")
        written.extend(files)
    return written


def data_generator(java: str, jar: Path, cwd: Path, log: Path, mode: str, *args: str) -> None:
    """Run `jar`'s data generator in `mode` from `cwd`, keeping everything it prints in `log`."""
    cwd.mkdir(parents=True, exist_ok=True)
    run = run_tool(
        [java, "-DbundlerMainClass=net.minecraft.data.Main", "-jar", str(jar), mode, *args],
        cwd=cwd,
    )
    output = run.stdout + run.stderr
    log.write_text(output, encoding="utf-8")
    if run.returncode != 0 or any(mark in output for mark in GENERATOR_FAILURES):
        tail = "\n".join(output.strip().splitlines()[-30:])
        raise Refused(
            f"the data generator's {mode} run failed (exit {run.returncode}, log in {log}):\n{tail}"
        )


# A state the game always saves with properties, so a reload that adds none never loaded it.
SENTINEL = {
    "size": [1, 1, 1],
    "palette": [{"Name": "minecraft:oak_stairs"}],
    "blocks": [{"pos": [0, 0, 0], "state": 0}],
    "entities": [],
}


def check_target(cairn: str, java: str, target: Target, cache: Path, work: Path) -> list[str]:
    """Report lines for what the game reads differently from what `cairn` writes for `target`.

    At most `SHOWN_PER_FILE` lines are kept per structure, and how many differ is printed first.
    """
    jar, needs_java = server_jar(target.mc_version, cache)
    has_java = java_major(java)
    if has_java < needs_java:
        raise Refused(
            f"Java {target.mc_version} needs Java {needs_java}, and `{java}` is {has_java}"
        )
    root = work / target.mc_version
    try:
        if root.exists():
            shutil.rmtree(root)
    except OSError as problem:
        raise Refused(f"cannot clear the last run's {root}: {problem}") from None
    structures = root / "in" / "data" / "minecraft" / target.structure_folder()
    files = compile_examples(cairn, target, structures, root / "locks")
    sentinel = structures / "sentinel.nbt"
    sentinel.write_bytes(write_nbt({**SENTINEL, "DataVersion": target.data_version}))
    for mode, source, output in (("--dev", "in", "snbt"), ("--server", "snbt", "out")):
        data_generator(
            java, jar, root / "runtime", root / f"{mode[2:]}.log", mode,
            "--input", str(root / source), "--output", str(root / output),
        )

    def copy_of(path: Path) -> Path:
        return root / "out" / path.relative_to(root / "in")

    if not copy_of(sentinel).exists():
        raise Refused(
            f"the {target.mc_version} data generator wrote no copy of the sentinel structure "
            f"(logs in {root})"
        )
    try:
        reloaded = block_states(read_nbt(copy_of(sentinel).read_bytes()))
    except (OSError, EOFError, ValueError, LookupError) as problem:
        raise Refused(f"the game's copy of the sentinel does not read: {problem}") from None
    if not reloaded.get((0, 0, 0), ("", {}))[1]:
        raise Refused(
            f"the {target.mc_version} data generator converted the structures without loading "
            f"them; it reads structures from a folder other than `{target.structure_folder()}`"
        )
    found = []
    differing = 0
    for path in files:
        where = f"Java {target.mc_version} {path.parent.name}/{path.name}"
        lines = compare(path, copy_of(path))
        differing += bool(lines)
        found.extend(f"{where}: {line}" for line in lines[:SHOWN_PER_FILE])
        if len(lines) > SHOWN_PER_FILE:
            found.append(f"{where}: and {len(lines) - SHOWN_PER_FILE} more")
    print(
        f"Java {target.mc_version}: {len(files)} structures, {differing} read differently",
        flush=True,
    )
    return found


def main(argv: list[str] | None = None) -> int:
    """0 when the game reads every structure as written, 1 when not or refused, 2 on misuse."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--cairn", required=True, help="the `cairn` binary to compile with")
    parser.add_argument("--java", default="java", help="a Java new enough for every target")
    parser.add_argument(
        "--cache",
        type=Path,
        default=Path.home() / ".cache" / "cairn-minecraft",
        help="where the server jars are kept",
    )
    parser.add_argument(
        "--work", type=Path, help="where to compile and convert; a new temporary folder by default"
    )
    parser.add_argument(
        "--target",
        action="append",
        help="check only this version (repeatable); every shipped one by default",
    )
    args = parser.parse_args(argv)

    targets = shipped_targets(json.loads(DATA_VERSIONS.read_text(encoding="utf-8")))
    if args.target:
        unknown = sorted(set(args.target) - {t.mc_version for t in targets})
        if unknown:
            print(f"not a shipped Java target: {', '.join(unknown)}", file=sys.stderr)
            return 2
        targets = [t for t in targets if t.mc_version in args.target]
    # Windows starts no relative path with `/` and no `./`; a bare name stays a PATH lookup.
    cairn, java = os.path.normpath(args.cairn), os.path.normpath(args.java)
    cache = args.cache.resolve()
    work = (args.work or Path(tempfile.mkdtemp(prefix="cairn-minecraft-load-"))).resolve()
    differ = False
    try:
        if not targets:
            raise Refused(f"{shown(DATA_VERSIONS)} has no targetable Java version")
        for target in targets:
            lines = check_target(cairn, java, target, cache, work)
            for line in lines:
                print(line, file=sys.stderr, flush=True)
            differ = differ or bool(lines)
    except Refused as refusal:
        print(f"error: {refusal}", file=sys.stderr, flush=True)
        return 1
    return 1 if differ else 0


if __name__ == "__main__":
    sys.exit(main())
