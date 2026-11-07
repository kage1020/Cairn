#!/usr/bin/env python3
"""Extract the built-in registry packs' block tables from each edition's own game data.

For each targetable row of an edition's `data_versions.json`, the game names its block ids and
the values each of their state properties takes:

- Java: the version's server jar, as Mojang's version manifest publishes it, runs its data
  generator with `--reports`. `reports/blocks.json` lists every block and its properties, and the
  jar's `version.json` its `DataVersion`.
- Bedrock: the game's block palette, every state the release accepts, as GeyserMC ships it for that
  protocol (pinned below by commit and SHA-256). Its `version` is the block-state version a
  structure written for the release carries.

The run rewrites the edition's `blocks.json`, the targetable rows' integers and `latest` in
`data_versions.json`, and the `extracted` record in `pack.json`. With `--check` it writes nothing
and fails when a committed file differs from what the game says.

Standard library only: it runs on a bare `ubuntu-latest` runner.
"""

from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import os
import sys
import tempfile
import zipfile
from dataclasses import dataclass
from pathlib import Path

from minecraft_load import (
    Byte,
    Refused,
    data_generator,
    fetch,
    java_major,
    read_nbt,
    server_download,
    server_jar,
    write_atomically,
)

ROOT = Path(__file__).resolve().parents[2]
PACKS = ROOT / "crates/cairn-lang-formats/registry-data"
NAMESPACE = "minecraft"
BLOCKS_SCHEMA = 2
DATA_VERSIONS_SCHEMA = 3

GEYSER_PALETTE = (
    "https://raw.githubusercontent.com/GeyserMC/Geyser/{commit}/core/src/main/resources/bedrock/{file}"
)

# Geyser adds a protocol's palette when it supports the release and deletes it when it drops the
# release, so each pin is the commit that added the file.
BEDROCK_PALETTES = {
    "1.21.0": (
        "fa6808a62029373af4492329bbf2cbf80bb51e7f",
        "block_palette.1_21_0.nbt",
        "a866c3b757ed8c89a2a2084b41e20782dc5cc210e7f9a20782e22e4a39f9f263",
    ),
    "1.21.40": (
        "fb868e41ed35dc079a63b5b6623c220f64c6c82b",
        "block_palette.1_21_40.nbt",
        "53deb7bcf93de6104a7b3913afd2c7151b751eb43156643d7c6b9272ad11a792",
    ),
    "1.21.60": (
        "b8477aa40190172ce56031d15edbf17f69e74bb6",
        "block_palette.1_21_60.nbt",
        "b4885756f1f34ad27ab602131c9df2a523ebc21176b2217e4436f84174274588",
    ),
    "1.26.60": (
        "fd35d5918ae8e8b6c05d91179315fa00413c1f65",
        "block_palette.26_60.nbt",
        "9d0feee0cd2edc1499bd7f7922615caee4dd268b598fca45dc396f4d0bdcdaa3",
    ),
}

DIFF_LINES_SHOWN = 40

# A property's values, in the game's order: strings on Java; a bool, int or string on Bedrock.
Domain = list
States = dict[str, Domain]


@dataclass(frozen=True)
class Table:
    """One version's blocks as the game declares them."""

    mc_version: str
    # Bare id → property → domain, ids and properties in name order.
    blocks: dict[str, States]
    # Java's `DataVersion`, or the block-state version Bedrock's palette carries.
    version_int: int
    source: dict


def bare(identifier: str) -> str:
    """`identifier` without the namespace every built-in table shares."""
    namespace, _, name = identifier.partition(":")
    if namespace != NAMESPACE or not name:
        raise Refused(f"`{identifier}` is not a `{NAMESPACE}:` id")
    return name


def java_blocks(report: dict) -> dict[str, States]:
    """The ids and domains of a `--reports` `blocks.json`, keeping each property's value order."""
    if not report:
        raise Refused("the block report lists no block")
    blocks = {}
    for identifier, block in report.items():
        properties = block.get("properties", {})
        blocks[bare(identifier)] = {name: list(values) for name, values in sorted(properties.items())}
    return dict(sorted(blocks.items()))


def extract_java(version: str, java: str, cache: Path, work: Path) -> Table:
    """The block table of the Java server jar for `version`."""
    download, needs_java = server_download(version, cache)
    has_java = java_major(java)
    if has_java < needs_java:
        raise Refused(f"Java {version} needs Java {needs_java}, and `{java}` is {has_java}")
    jar, _ = server_jar(version, cache)
    root = work / version
    data_generator(
        java, jar, root / "runtime", root / "reports.log", "--reports", "--output", str(root / "out")
    )
    report = root / "out" / "reports" / "blocks.json"
    try:
        blocks = java_blocks(json.loads(report.read_text(encoding="utf-8")))
        with zipfile.ZipFile(jar) as archive:
            data_version = int(json.loads(archive.read("version.json"))["world_version"])
    except (OSError, ValueError, KeyError, zipfile.BadZipFile) as problem:
        raise Refused(f"the {version} server's reports do not read: {problem}") from None
    return Table(version, blocks, data_version, {"url": download["url"], "sha1": download["sha1"]})


def bedrock_value(tag_value) -> bool | int | str:
    """A palette state value as the table writes it: a `Byte` is Bedrock's boolean."""
    if isinstance(tag_value, Byte):
        if tag_value not in (0, 1):
            raise Refused(f"a byte state holds {int(tag_value)}, which is no boolean")
        return bool(tag_value)
    if isinstance(tag_value, (int, str)):
        return tag_value if isinstance(tag_value, str) else int(tag_value)
    raise Refused(f"a palette state holds {tag_value!r}, which is no bool, int or string")


def bedrock_blocks(palette: dict) -> tuple[dict[str, States], int]:
    """The ids and domains of a block palette, and the one block-state version all of it carries."""
    blocks: dict[str, dict[str, list]] = {}
    versions = set()
    entries = palette.get("blocks", [])
    if not entries:
        raise Refused("the palette lists no block")
    for entry in entries:
        states = blocks.setdefault(bare(entry["name"]), {})
        versions.add(int(entry["version"]))
        for name, raw in entry.get("states", {}).items():
            domain = states.setdefault(name, [])
            value = bedrock_value(raw)
            # `True == 1`, so a membership test alone would merge a bool and an int domain.
            if not any(type(v) is type(value) and v == value for v in domain):
                domain.append(value)
    if len(versions) != 1:
        raise Refused(f"the palette carries {len(versions)} block-state versions, not one")
    tidy = {
        name: {prop: states[prop] for prop in sorted(states)} for name, states in blocks.items()
    }
    return dict(sorted(tidy.items())), versions.pop()


def extract_bedrock(version: str, cache: Path) -> Table:
    """The block table of the pinned Bedrock palette for `version`."""
    if version not in BEDROCK_PALETTES:
        raise Refused(f"no Bedrock palette is pinned for {version}; add it to BEDROCK_PALETTES")
    commit, file, sha256 = BEDROCK_PALETTES[version]
    url = GEYSER_PALETTE.format(commit=commit, file=file)
    cache.mkdir(parents=True, exist_ok=True)
    cached = cache / f"bedrock-{version}-{sha256[:12]}.nbt"
    raw = cached.read_bytes() if cached.exists() else b""
    if hashlib.sha256(raw).hexdigest() != sha256:
        raw = fetch(url, 300)
        if hashlib.sha256(raw).hexdigest() != sha256:
            raise Refused(
                f"the Bedrock {version} palette at {url} does not match its pinned SHA-256"
            )
        write_atomically(cached, raw)
    try:
        blocks, block_version = bedrock_blocks(read_nbt(raw))
    except (ValueError, KeyError, EOFError) as problem:
        raise Refused(f"the Bedrock {version} palette does not read: {problem}") from None
    return Table(version, blocks, block_version, {"url": url, "sha256": sha256})


# --- the pack files -----------------------------------------------------------------------------


def entry_lines(blocks: dict[str, States], indent: int) -> list[str]:
    """One `"id": {property: domain}` line per block, comma-joined."""
    pad = " " * indent
    lines = [f"{pad}{json.dumps(name)}: {json.dumps(states)}" for name, states in blocks.items()]
    return [line + ("," if i < len(lines) - 1 else "") for i, line in enumerate(lines)]


def object_lines(key: str, blocks: dict[str, States], indent: int, last: bool) -> list[str]:
    """`key` holding `blocks` one per line, or `{}` when there are none."""
    pad = " " * indent
    tail = "" if last else ","
    if not blocks:
        return [f"{pad}{json.dumps(key)}: {{}}{tail}"]
    return [f"{pad}{json.dumps(key)}: {{", *entry_lines(blocks, indent + 2), f"{pad}}}{tail}"]


def blocks_text(tables: list[Table]) -> str:
    """`blocks.json`: the first table whole, then each later one as a diff of the one before it."""
    base, *later = tables
    lines = [
        "{",
        f'  "schema_version": {BLOCKS_SCHEMA},',
        f'  "namespace": {json.dumps(NAMESPACE)},',
        '  "base": {',
        f'    "mc_version": {json.dumps(base.mc_version)},',
        *object_lines("blocks", base.blocks, 4, last=True),
        "  },",
        '  "diffs": [' if later else '  "diffs": []',
    ]
    for i, (parent, table) in enumerate(zip(tables, later)):
        added = {k: v for k, v in table.blocks.items() if k not in parent.blocks}
        # `True == 1`, so comparing the lists would miss a boolean domain retyped as integers.
        changed = {
            k: v
            for k, v in table.blocks.items()
            if k in parent.blocks and json.dumps(parent.blocks[k]) != json.dumps(v)
        }
        removed = [k for k in parent.blocks if k not in table.blocks]
        lines += [
            "    {",
            f'      "mc_version": {json.dumps(table.mc_version)},',
            f'      "inherits": {json.dumps(parent.mc_version)},',
            *object_lines("added", added, 6, last=False),
            *object_lines("changed", changed, 6, last=False),
            f'      "removed": {json.dumps(removed)}',
            "    }" + ("," if i < len(later) - 1 else ""),
        ]
    if later:
        lines.append("  ]")
    lines.append("}")
    return "\n".join(lines) + "\n"


def json_text(value: dict) -> str:
    """`value` as the pack's hand-edited files lay JSON out."""
    return json.dumps(value, indent=2, ensure_ascii=False) + "\n"


def targetable_rows(table: dict) -> list[dict]:
    """The rows `--target` may name, in table order."""
    return [row for row in table["versions"] if row.get("targetable", True)]


def updated_data_versions(table: dict, edition: str, tables: list[Table]) -> dict:
    """`table` with each targetable row's integer taken from the game, and `latest` the newest."""
    by_version = {t.mc_version: t for t in tables}
    field = "data_version" if edition == "java" else "block_version"
    rows = []
    for row in table["versions"]:
        if row.get("targetable", True):
            row = {**row, field: by_version[row["mc_version"]].version_int}
        # The game's integers first, then what a person writes.
        order = ("mc_version", "data_version", "block_version", "released", "targetable")
        rows.append({k: row[k] for k in (*order, *row) if k in row})
    ordered = {"schema_version": DATA_VERSIONS_SCHEMA, "latest": tables[-1].mc_version}
    ordered.update({k: v for k, v in table.items() if k not in ("schema_version", "latest", "versions")})
    ordered["versions"] = rows
    return ordered


def updated_manifest(manifest: dict, tables: list[Table]) -> dict:
    """`manifest` recording where each block table came from."""
    script = Path(__file__).resolve().relative_to(ROOT).as_posix()
    return {
        **manifest,
        "extracted": {
            "by": script,
            "blocks": [{"mc_version": t.mc_version, **t.source} for t in tables],
        },
    }


def pack_files(edition: str, tables: list[Table]) -> dict[Path, str]:
    """The text each extracted pack file should hold."""
    pack = PACKS / edition
    manifest = json.loads((pack / "pack.json").read_text(encoding="utf-8"))
    versions = json.loads((pack / "data_versions.json").read_text(encoding="utf-8"))
    return {
        pack / manifest["files"]["blocks"]: blocks_text(tables),
        pack / manifest["files"]["data_versions"]: json_text(
            updated_data_versions(versions, edition, tables)
        ),
        pack / "pack.json": json_text(updated_manifest(manifest, tables)),
    }


def diff_head(name: str, committed: str, wanted: str) -> list[str]:
    """The first lines of the unified diff from `committed` to `wanted`, and a count of the rest."""
    lines = [
        line if line.endswith("\n") else line + "\n"
        for line in difflib.unified_diff(
            committed.splitlines(keepends=True),
            wanted.splitlines(keepends=True),
            f"{name} (committed)",
            f"{name} (from the game)",
        )
    ]
    head = lines[:DIFF_LINES_SHOWN]
    if len(lines) > DIFF_LINES_SHOWN:
        head.append(f"... and {len(lines) - DIFF_LINES_SHOWN} more lines\n")
    return head


def extract(edition: str, java: str, cache: Path, work: Path) -> list[Table]:
    """Every targetable version of `edition`'s pack, extracted, in table order."""
    table = json.loads((PACKS / edition / "data_versions.json").read_text(encoding="utf-8"))
    versions = [row["mc_version"] for row in targetable_rows(table)]
    if not versions:
        raise Refused(f"the {edition} pack has no targetable version")
    tables = []
    for version in versions:
        if edition == "java":
            tables.append(extract_java(version, java, cache, work))
        else:
            tables.append(extract_bedrock(version, cache))
        print(f"{edition} {version}: {len(tables[-1].blocks)} blocks", flush=True)
    return tables


def main(argv: list[str] | None = None) -> int:
    """0 when the files hold, or now hold, what the game says; 1 on a refusal or, with `--check`, a
    difference; 2 on misuse."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("edition", choices=("java", "bedrock"))
    parser.add_argument("--java", default="java", help="a Java new enough for every Java target")
    parser.add_argument(
        "--cache",
        type=Path,
        default=Path.home() / ".cache" / "cairn-minecraft",
        help="where the server jars and palettes are kept",
    )
    parser.add_argument(
        "--check", action="store_true", help="write nothing; fail when a file differs"
    )
    args = parser.parse_args(argv)
    try:
        with tempfile.TemporaryDirectory(prefix="cairn-extract-registry-") as work:
            java = os.path.normpath(args.java)
            tables = extract(args.edition, java, args.cache.resolve(), Path(work).resolve())
        files = pack_files(args.edition, tables)
    except Refused as refusal:
        print(f"error: {refusal}", file=sys.stderr, flush=True)
        return 1
    committed = {path: path.read_text(encoding="utf-8") for path in files}
    stale = [path for path, text in files.items() if committed[path] != text]
    if args.check:
        for path in stale:
            name = path.relative_to(ROOT).as_posix()
            print(f"{name} differs from what the game says:", file=sys.stderr)
            sys.stderr.writelines(diff_head(name, committed[path], files[path]))
        return 1 if stale else 0
    for path in stale:
        write_atomically(path, files[path].encode("utf-8"))
        print(f"wrote {path.relative_to(ROOT)}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
