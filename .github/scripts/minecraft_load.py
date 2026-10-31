#!/usr/bin/env python3
"""Load every example's `.nbt` with Minecraft Java's own structure loader, at each shipped target.

For each targetable row of the built-in Java pack, every `examples/*.crn` is compiled with
`cairn compile --edition java --target <version>`, and the vanilla server jar of that version
reads each `.nbt` back through its data generator: `--dev` turns the files into SNBT, and
`--server` turns the SNBT back into structures through `StructureUpdater`, which runs the
DataFixer from the file's `DataVersion`, loads the template against the block registry and saves
it again. That is the load `/place template` and a structure block perform. A block the game does
not know comes back as air, a property it does not know is dropped, a value outside its domain
reverts to the default, and an older `DataVersion` is upgraded, so any difference between a
written file and its reloaded copy is a file the game reads differently from what was written.

The server jar is downloaded from Mojang at run time, checked against the SHA-1 Mojang publishes,
and kept in the cache directory; it is never committed or redistributed, and its use is under the
Minecraft EULA. The data generator writes no world and needs no `eula.txt`.

Standard library only: it runs on a bare `ubuntu-latest` runner.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
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

# How many differences to print for one structure before summarising the rest.
SHOWN_PER_FILE = 5

TAG_END, TAG_BYTE, TAG_SHORT, TAG_INT, TAG_LONG = 0, 1, 2, 3, 4
TAG_FLOAT, TAG_DOUBLE, TAG_BYTE_ARRAY, TAG_STRING = 5, 6, 7, 8
TAG_LIST, TAG_COMPOUND, TAG_INT_ARRAY, TAG_LONG_ARRAY = 9, 10, 11, 12


class Refused(Exception):
    """A step the check depends on did not complete."""


# --- NBT ------------------------------------------------------------------------------------


def read_nbt(raw: bytes) -> dict:
    """The root compound of a gzip-compressed, big-endian NBT file."""
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
        if tag in (TAG_BYTE, TAG_SHORT, TAG_INT, TAG_LONG, TAG_FLOAT, TAG_DOUBLE):
            return unpack({1: ">b", 2: ">h", 3: ">i", 4: ">q", 5: ">f", 6: ">d"}[tag])
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
    """`root` as a gzip-compressed NBT file: ints as `Int`, strings, lists and compounds only."""
    out = io.BytesIO()

    def tag_of(value) -> int:
        if isinstance(value, bool) or not isinstance(value, (int, str, list, dict)):
            raise TypeError(f"no NBT tag for {value!r}")
        return {int: TAG_INT, str: TAG_STRING, list: TAG_LIST, dict: TAG_COMPOUND}[type(value)]

    def string(text: str) -> None:
        encoded = text.encode("utf-8")
        out.write(struct.pack(">H", len(encoded)) + encoded)

    def payload(value) -> None:
        tag = tag_of(value)
        if tag == TAG_INT:
            out.write(struct.pack(">i", value))
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


def block_states(structure: dict) -> dict[tuple[int, int, int], tuple[str, dict, object]]:
    """Each block of a structure by position, as its id, properties and block-entity NBT."""
    palette = structure["palette"] if "palette" in structure else structure["palettes"][0]
    states = {}
    for block in structure.get("blocks", []):
        entry = palette[block["state"]]
        states[tuple(block["pos"])] = (
            entry["Name"],
            entry.get("Properties", {}),
            block.get("nbt"),
        )
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
    if len(written.get("entities", [])) != len(loaded.get("entities", [])):
        found.append(
            f"{len(written.get('entities', []))} entities were read as "
            f"{len(loaded.get('entities', []))}"
        )
    ours, theirs = block_states(written), block_states(loaded)
    for pos in sorted(ours.keys() | theirs.keys()):
        if pos not in theirs:
            found.append(f"{pos}: {ours[pos][0]} was dropped")
            continue
        if pos not in ours:
            found.append(f"{pos}: {theirs[pos][0]} appeared")
            continue
        name, properties, entity = ours[pos]
        read_name, read_properties, read_entity = theirs[pos]
        if name != read_name:
            found.append(f"{pos}: {name} was read as {read_name}")
            continue
        for key, value in properties.items():
            if read_properties.get(key) != value:
                read = read_properties.get(key)
                shown = "dropped" if read is None else f"read as {key}={read}"
                found.append(f"{pos}: {name}[{key}={value}] {shown}")
        if entity is not None and read_entity is None:
            found.append(f"{pos}: {name}'s block entity was dropped")
    return found


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


def fetch_json(url: str) -> dict:
    with urllib.request.urlopen(url, timeout=60) as response:
        return json.load(response)


def server_jar(version: str, cache: Path) -> tuple[Path, int]:
    """The vanilla server jar for `version` in `cache`, and the Java major version it needs."""
    cache.mkdir(parents=True, exist_ok=True)
    meta_path = cache / f"{version}.json"
    if not meta_path.exists():
        manifest = fetch_json(MANIFEST_URL)
        urls = [v["url"] for v in manifest["versions"] if v["id"] == version]
        if not urls:
            raise Refused(f"Mojang's version manifest has no {version}")
        meta_path.write_text(json.dumps(fetch_json(urls[0])))
    meta = json.loads(meta_path.read_text())
    download = meta["downloads"]["server"]
    jar = cache / f"server-{version}.jar"
    if not jar.exists() or hashlib.sha1(jar.read_bytes()).hexdigest() != download["sha1"]:
        with urllib.request.urlopen(download["url"], timeout=300) as response:
            body = response.read()
        if hashlib.sha1(body).hexdigest() != download["sha1"]:
            raise Refused(f"the {version} server jar does not match the SHA-1 Mojang publishes")
        jar.write_bytes(body)
    return jar, int(meta["javaVersion"]["majorVersion"])


def java_major(java: str) -> int:
    """The major version of the `java` executable."""
    shown = subprocess.run(
        [java, "-XshowSettings:properties", "-version"], capture_output=True, text=True
    ).stderr
    for line in shown.splitlines():
        if line.strip().startswith("java.specification.version"):
            return int(line.split("=")[1].strip().split(".")[-1])
    raise Refused(f"cannot tell which Java `{java}` is")


# --- the check -------------------------------------------------------------------------------


def compile_examples(cairn: str, target: Target, into: Path, locks: Path) -> list[Path]:
    """Every example compiled for `target` under `into`, one folder per example."""
    locks.mkdir(parents=True, exist_ok=True)
    written = []
    for source in sorted(EXAMPLES.glob("*.crn")):
        out = into / source.stem
        run = subprocess.run(
            [
                cairn, "compile", "--edition", "java", "--target", target.mc_version,
                "--out", str(out), "--lock", str(locks / f"{source.stem}.lock"), str(source),
            ],
            capture_output=True,
            text=True,
        )
        if run.returncode != 0 or run.stderr.strip():
            raise Refused(
                f"{source.relative_to(ROOT)} does not compile clean for Java "
                f"{target.mc_version} (exit {run.returncode}):\n{run.stderr.strip()}"
            )
        files = sorted(out.glob("*.nbt"))
        if not files:
            raise Refused(f"{source.relative_to(ROOT)} wrote no .nbt for Java {target.mc_version}")
        written.extend(files)
    return written


def data_generator(java: str, jar: Path, cwd: Path, *args: str) -> None:
    cwd.mkdir(parents=True, exist_ok=True)
    run = subprocess.run(
        [java, "-DbundlerMainClass=net.minecraft.data.Main", "-jar", str(jar), *args],
        cwd=cwd,
        capture_output=True,
        text=True,
    )
    if run.returncode != 0:
        log = (run.stdout + run.stderr).strip().splitlines()
        tail = "\n".join(log[-30:])
        raise Refused(f"the data generator failed (exit {run.returncode}):\n{tail}")


# A state the game always saves with properties, so a reload that adds none never loaded it.
SENTINEL = {
    "size": [1, 1, 1],
    "palette": [{"Name": "minecraft:oak_stairs"}],
    "blocks": [{"pos": [0, 0, 0], "state": 0}],
    "entities": [],
}


def check_target(cairn: str, java: str, target: Target, cache: Path, work: Path) -> list[str]:
    """Every difference between what `cairn` writes for `target` and what the game loads."""
    jar, needs_java = server_jar(target.mc_version, cache)
    has_java = java_major(java)
    if has_java < needs_java:
        raise Refused(
            f"Java {target.mc_version} needs Java {needs_java}, and `{java}` is {has_java}"
        )
    root = work / target.mc_version
    shutil.rmtree(root, ignore_errors=True)
    structures = root / "in" / "data" / "minecraft" / target.structure_folder()
    files = compile_examples(cairn, target, structures, root / "locks")
    sentinel = structures / "sentinel.nbt"
    sentinel.write_bytes(write_nbt({**SENTINEL, "DataVersion": target.data_version}))
    # The bundler unpacks its libraries into the working directory, so that stays in the cache.
    runtime = cache / target.mc_version
    for mode, source, output in (("--dev", "in", "snbt"), ("--server", "snbt", "out")):
        data_generator(
            java, jar, runtime, mode, "--input", str(root / source), "--output", str(root / output)
        )

    def loaded(path: Path) -> dict | None:
        copy = root / "out" / path.relative_to(root / "in")
        return read_nbt(copy.read_bytes()) if copy.exists() else None

    reloaded_sentinel = loaded(sentinel)
    if reloaded_sentinel is None or not block_states(reloaded_sentinel)[(0, 0, 0)][1]:
        raise Refused(
            f"the {target.mc_version} data generator converted the structures without loading "
            f"them; it reads structures from a folder other than `{target.structure_folder()}`"
        )
    found = []
    differing = 0
    for path in files:
        where = f"Java {target.mc_version} {path.parent.name}/{path.name}"
        copy = loaded(path)
        lines = (
            ["the game wrote no copy of it"]
            if copy is None
            else differences(read_nbt(path.read_bytes()), copy)
        )
        differing += bool(lines)
        found.extend(f"{where}: {line}" for line in lines[:SHOWN_PER_FILE])
        if len(lines) > SHOWN_PER_FILE:
            found.append(f"{where}: and {len(lines) - SHOWN_PER_FILE} more")
    print(f"Java {target.mc_version}: {len(files)} structures, {differing} read differently")
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--cairn", required=True, help="the `cairn` binary to compile with")
    parser.add_argument("--java", default="java", help="a Java new enough for every target")
    parser.add_argument(
        "--cache",
        type=Path,
        default=Path.home() / ".cache" / "cairn-minecraft",
        help="where the server jars and their unpacked libraries are kept",
    )
    parser.add_argument(
        "--work",
        type=Path,
        default=Path(tempfile.gettempdir()) / "cairn-minecraft-load",
        help="where to compile and convert",
    )
    parser.add_argument(
        "--target",
        action="append",
        help="check only this version (repeatable); every shipped one by default",
    )
    args = parser.parse_args()

    targets = shipped_targets(json.loads(DATA_VERSIONS.read_text()))
    if args.target:
        unknown = sorted(set(args.target) - {t.mc_version for t in targets})
        if unknown:
            print(f"not a shipped Java target: {', '.join(unknown)}", file=sys.stderr)
            return 2
        targets = [t for t in targets if t.mc_version in args.target]
    cache, work = args.cache.resolve(), args.work.resolve()
    found = []
    try:
        for target in targets:
            found.extend(check_target(args.cairn, args.java, target, cache, work))
    except Refused as refusal:
        print(f"error: {refusal}", file=sys.stderr)
        return 1
    for line in found:
        print(line, file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
