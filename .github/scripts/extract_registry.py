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

The Bedrock pack also gets its state maps: how the canonical vocabulary's states, which are Java's,
become each release's own. GeyserMC maps every state of one Java release to a Bedrock block and
states (pinned below too); the Java states that land on one Bedrock block form a family, each
Bedrock state is factored into a lookup over the fewest Java properties that decide it, and each
family is checked against the release's palette. Reading a mapping takes its Java release's own
`--reports`, so the Bedrock run needs Java too. A Java property no Bedrock state reads must be
classified by hand in the pack's `sensitivity.json`.

The run rewrites the edition's `blocks.json`, the targetable rows' integers and `latest` in
`data_versions.json`, Bedrock's `states.json`, and the `extracted` record in `pack.json`. With
`--check` it writes nothing and fails when a committed file differs from what the game says.

Standard library only: it runs on a bare `ubuntu-latest` runner.
"""

from __future__ import annotations

import argparse
import difflib
import hashlib
import itertools
import json
import math
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
STATES_SCHEMA = 1
SENSITIVITY_SCHEMA = 1

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

GEYSER_MAPPINGS = "https://raw.githubusercontent.com/GeyserMC/mappings/{commit}/blocks.nbt"

# Each Bedrock release's state map: a GeyserMC mappings commit, its SHA-256, and the Java release
# whose block states, in id order, its entries follow. Geyser snapshots only the releases it maps
# natively and converts the rest in code: 1.21.40 reads 1.21.30's snapshot, as Geyser's own 1.21.40
# release did, and 1.21.60 reads 1.21.70's, the nearest one whose states its palette accepts.
BEDROCK_STATE_MAPS = {
    "1.21.0": (
        "ec45f59c8590945c9226921ef7e339f510983dc1",
        "efab40e80405474356e6940d3bac9dd0f904b1633a36bff3312472a16ace8821",
        "1.21",
    ),
    "1.21.40": (
        "830085e37e0053c9d6116bcad37cafcba83aded5",
        "e48d4f6b3f60a23707f9cb35ebebf000b41f665b2295d65a6692d37ab1fc0768",
        "1.21",
    ),
    "1.21.60": (
        "3fcfc7bab4345b6807d7507a02e52ed4e2da8b31",
        "de65e6382d74e27b72e8946b39ffbd6ff8aae39e9ec6e3193a5958187be333bf",
        "1.21.5",
    ),
    "1.26.60": (
        "b1934f14ad6fba5ea00ee2938e1f8cbef48e6b12",
        "efc9305b53fc170dd153eb417e003a61f2b17f924592892726ecffc3dce58536",
        "26.3",
    ),
}

# The Java state the `.mcstructure` writer carries as the water layer rather than as a state.
WATER_LAYER_STATE = "waterlogged"

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


@dataclass(frozen=True)
class JavaReport:
    """What one Java server jar's data generator says about its blocks."""

    mc_version: str
    # `reports/blocks.json` as the generator writes it.
    blocks: dict
    data_version: int
    source: dict


def java_report(version: str, java: str, cache: Path, work: Path) -> JavaReport:
    """The `--reports` blocks of the Java server jar for `version`, generated once per run."""
    download, needs_java = server_download(version, cache)
    has_java = java_major(java)
    if has_java < needs_java:
        raise Refused(f"Java {version} needs Java {needs_java}, and `{java}` is {has_java}")
    jar, _ = server_jar(version, cache)
    root = work / version
    report = root / "out" / "reports" / "blocks.json"
    if not report.exists():
        data_generator(
            java, jar, root / "runtime", root / "reports.log", "--reports", "--output", str(root / "out")
        )
    try:
        blocks = json.loads(report.read_text(encoding="utf-8"))
        with zipfile.ZipFile(jar) as archive:
            data_version = int(json.loads(archive.read("version.json"))["world_version"])
    except (OSError, ValueError, KeyError, zipfile.BadZipFile) as problem:
        raise Refused(f"the {version} server's reports do not read: {problem}") from None
    return JavaReport(
        version, blocks, data_version, {"url": download["url"], "sha1": download["sha1"]}
    )


def extract_java(version: str, java: str, cache: Path, work: Path) -> Table:
    """The block table of the Java server jar for `version`."""
    report = java_report(version, java, cache, work)
    return Table(version, java_blocks(report.blocks), report.data_version, report.source)


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


# --- the Bedrock state map ----------------------------------------------------------------------

# A Java block state: its bare id and its properties.
JavaState = tuple[str, dict[str, str]]
# What Geyser maps one Java state to: Bedrock's states, valued as the palette types them.
BedrockStates = dict[str, bool | int | str]
# Geyser's stand-in for a Java block Bedrock has no block for, which is not the block's image.
PLACEHOLDER = "unknown"


@dataclass(frozen=True)
class StateRule:
    """How one Bedrock state is set: looked up from the Java values of `reads`, or a constant."""

    reads: tuple[str, ...]
    # The `reads` values, comma-joined, to the Bedrock value; one `""` key when `reads` is empty.
    table: tuple[tuple[str, bool | int | str], ...]


@dataclass(frozen=True)
class Family:
    """The Java states of one block that become one Bedrock block, and how their states map."""

    # Every property of the Java block, with the values these states hold, in Java's order.
    java: tuple[tuple[str, tuple[str, ...]], ...]
    # The Java block's default state.
    defaults: tuple[tuple[str, str], ...]
    states: tuple[tuple[str, StateRule], ...]
    # The properties held at more than one value that no Bedrock state reads, but the water layer.
    dropped: tuple[str, ...]


def geyser_mappings(version: str, cache: Path) -> list[dict]:
    """GeyserMC's Bedrock state for each Java state, in Java id order, for Bedrock `version`."""
    if version not in BEDROCK_STATE_MAPS:
        raise Refused(f"no state map is pinned for Bedrock {version}; add it to BEDROCK_STATE_MAPS")
    commit, sha256, _ = BEDROCK_STATE_MAPS[version]
    url = GEYSER_MAPPINGS.format(commit=commit)
    cache.mkdir(parents=True, exist_ok=True)
    cached = cache / f"geyser-mappings-{sha256[:12]}.nbt"
    raw = cached.read_bytes() if cached.exists() else b""
    if hashlib.sha256(raw).hexdigest() != sha256:
        raw = fetch(url, 300)
        if hashlib.sha256(raw).hexdigest() != sha256:
            raise Refused(f"the state map at {url} does not match its pinned SHA-256")
        write_atomically(cached, raw)
    try:
        return list(read_nbt(raw)["bedrock_mappings"])
    except (ValueError, KeyError, EOFError) as problem:
        raise Refused(f"the state map for Bedrock {version} does not read: {problem}") from None


@dataclass(frozen=True)
class JavaStates:
    """Every state of a `--reports` `blocks.json`, in id order, with each block's domains."""

    rows: list[JavaState]
    defaults: dict[str, dict[str, str]]
    domains: dict[str, States]


def java_states(report: dict) -> JavaStates:
    """The states, defaults and domains of a `--reports` `blocks.json`."""
    by_id: dict[int, JavaState] = {}
    defaults: dict[str, dict[str, str]] = {}
    domains: dict[str, States] = {}
    for identifier, block in report.items():
        name = bare(identifier)
        domains[name] = {k: list(v) for k, v in block.get("properties", {}).items()}
        for state in block["states"]:
            properties = dict(state.get("properties", {}))
            by_id[int(state["id"])] = (name, properties)
            if state.get("default"):
                defaults[name] = properties
    if sorted(by_id) != list(range(len(by_id))):
        raise Refused("the Java block states are not numbered 0 to n - 1")
    return JavaStates([by_id[i] for i in range(len(by_id))], defaults, domains)


def decided_by(rows: list[tuple[dict[str, str], BedrockStates]], keys: list[str], state: str):
    """The fewest of `keys` whose values decide `state` across `rows`, with the lookup, or None."""
    if any(state not in bedrock for _, bedrock in rows):
        return None
    for count in range(len(keys) + 1):
        for reads in itertools.combinations(keys, count):
            table: dict[str, bool | int | str] = {}
            for java, bedrock in rows:
                joined = ",".join(java[k] for k in reads)
                if table.setdefault(joined, bedrock[state]) != bedrock[state]:
                    break
            else:
                return StateRule(reads, tuple(sorted(table.items())))
    return None


def family_of(
    pair: str, rows: list[tuple[dict[str, str], BedrockStates]], java: JavaStates
) -> Family | None:
    """The family `rows` form, or None when a Bedrock state is not a function of their values."""
    java_id = pair.rpartition("=")[2]
    default, domains = java.defaults[java_id], java.domains[java_id]
    held = {k: [v for v in domains[k] if any(java[k] == v for java, _ in rows)] for k in domains}
    if len(rows) != math.prod(len(values) for values in held.values()):
        # An entry finds its family by each value it holds, which needs every combination of them.
        raise Refused(f"the Java states mapped as `{pair}` are no product of their values")
    varied = [k for k in sorted(held) if len(held[k]) > 1]
    states = []
    for state in sorted({name for _, bedrock in rows for name in bedrock}):
        rule = decided_by(rows, varied, state)
        if rule is None:
            return None
        states.append((state, rule))
    read = {k for _, rule in states for k in rule.reads}
    return Family(
        java=tuple((k, tuple(held[k])) for k in sorted(held)),
        defaults=tuple(sorted(default.items())),
        states=tuple(states),
        dropped=tuple(k for k in varied if k not in read and k != WATER_LAYER_STATE),
    )


def accepts(table: dict[str, States], block: str, family: Family) -> bool:
    """Whether the palette's `block` declares every state and value `family` writes."""
    domains = table.get(block)
    if domains is None:
        return False
    for state, rule in family.states:
        domain = domains.get(state)
        if domain is None:
            return False
        for _, value in rule.table:
            if not any(type(v) is type(value) and v == value for v in domain):
                return False
    return True


@dataclass(frozen=True)
class VersionMap:
    """One Bedrock version's state map."""

    # Each `(Bedrock id, Java id)` pair's family, for the pairs the palette accepts.
    families: dict[tuple[str, str], Family]
    # The other pairs, named as `states.json` names them, in name order.
    refused: list[str]


def pair_name(bedrock_id: str, java_id: str) -> str:
    """A pair as `states.json` names it: the Bedrock id, and `=` the Java id when that differs."""
    return bedrock_id if bedrock_id == java_id else f"{bedrock_id}={java_id}"


def version_families(java: JavaStates, mappings: list[dict], table: dict[str, States]) -> VersionMap:
    """The state map Geyser's `mappings` give against the release's palette `table`."""
    if len(mappings) != len(java.rows):
        raise Refused(f"the state map has {len(mappings)} entries for {len(java.rows)} Java states")
    groups: dict[tuple[str, str], list[tuple[dict[str, str], BedrockStates]]] = {}
    for (java_id, properties), entry in zip(java.rows, mappings):
        # Geyser leaves the namespace off some of its ids.
        named = entry.get("bedrock_identifier", java_id)
        bedrock_id = bare(named) if ":" in named else named
        states = {name: bedrock_value(raw) for name, raw in entry.get("state", {}).items()}
        groups.setdefault((bedrock_id, java_id), []).append((properties, states))
    families = {}
    refused = []
    for (bedrock_id, java_id), rows in sorted(groups.items()):
        if bedrock_id == PLACEHOLDER:
            continue
        pair = pair_name(bedrock_id, java_id)
        family = family_of(pair, rows, java)
        if family is not None and accepts(table, bedrock_id, family):
            families[(bedrock_id, java_id)] = family
        else:
            refused.append(pair)
    return VersionMap(families, refused)


def rule_json(rule: StateRule) -> dict:
    """A rule as `states.json` writes it."""
    if not rule.reads:
        return {"value": rule.table[0][1]}
    return {"reads": list(rule.reads), "map": dict(rule.table)}


def family_json(family: Family, versions: list[str], ids: list[str]) -> dict:
    """One `states.json` family row."""
    return {
        "mc_versions": versions,
        "ids": ids,
        "java": {k: list(values) for k, values in family.java},
        "defaults": dict(family.defaults),
        "states": {state: rule_json(rule) for state, rule in family.states},
        "dropped": list(family.dropped),
    }


def sensitivity_row(rows: list[dict], java_id: str) -> dict | None:
    """The row of one dropped state's classification that covers `java_id`, or None."""
    named = [row for row in rows if java_id in row.get("java", [])]
    return named[0] if named else next((row for row in rows if "java" not in row), None)


def check_sensitivity(maps: dict[str, VersionMap], sensitivity: dict) -> None:
    """Refuse a dropped state `sensitivity` does not classify, and a row nothing drops."""
    dropping: dict[str, set[str]] = {}
    for version_map in maps.values():
        for (_, java_id), family in version_map.families.items():
            for key in family.dropped:
                dropping.setdefault(key, set()).add(java_id)
    classified = sensitivity.get("dropped_states", {})
    problems = []
    for key in sorted(dropping):
        rows = classified.get(key, [])
        missing = sorted(j for j in dropping[key] if sensitivity_row(rows, j) is None)
        if missing:
            problems.append(f"`{key}` on {', '.join(f'`{j}`' for j in missing)} is unclassified")
    for key, rows in sorted(classified.items()):
        named = [j for row in rows for j in row.get("java", [])]
        if len(named) != len(set(named)) or sum("java" not in row for row in rows) > 1:
            problems.append(f"`{key}` classifies a block twice")
        stale = sorted(j for j in named if j not in dropping.get(key, set()))
        if stale:
            problems.append(f"no state map drops `{key}` on {', '.join(f'`{j}`' for j in stale)}")
        if any("java" not in row for row in rows) and not dropping.get(key, set()) - set(named):
            problems.append(f"`{key}`'s unnamed row covers no block")
    if problems:
        raise Refused("sensitivity.json: " + "; ".join(problems))


def states_text(maps: dict[str, VersionMap], sensitivity: dict) -> str:
    """`states.json`: one row per family and set of versions, with the ids it maps there."""
    check_sensitivity(maps, sensitivity)
    order = list(maps)
    held: dict[tuple[Family, str, str], list[str]] = {}
    for version, version_map in maps.items():
        for (bedrock_id, java_id), family in version_map.families.items():
            held.setdefault((family, bedrock_id, java_id), []).append(version)
    rows: dict[tuple[Family, tuple[str, ...]], list[str]] = {}
    for (family, bedrock_id, java_id), versions in held.items():
        rows.setdefault((family, tuple(versions)), []).append(pair_name(bedrock_id, java_id))
    ordered = sorted(
        rows.items(),
        key=lambda row: (order.index(row[0][1][0]), -len(row[0][1]), sorted(row[1])[0]),
    )
    refused = {version: m.refused for version, m in maps.items() if m.refused}
    lines = [
        "{",
        f'  "schema_version": {STATES_SCHEMA},',
        f'  "namespace": {json.dumps(NAMESPACE)},',
        f'  "refused": {json.dumps(refused)},',
        '  "families": [',
    ]
    for i, ((family, versions), ids) in enumerate(ordered):
        row = json.dumps(family_json(family, list(versions), sorted(ids)), ensure_ascii=False)
        lines.append(f"    {row}" + ("," if i < len(ordered) - 1 else ""))
    lines += ["  ]", "}"]
    return "\n".join(lines) + "\n"


def extract_state_maps(
    tables: list[Table], java: str, cache: Path, work: Path
) -> dict[str, VersionMap]:
    """Each Bedrock table's state map, read off its pinned Geyser mappings."""
    maps = {}
    for table in tables:
        mappings = geyser_mappings(table.mc_version, cache)
        _, _, java_version = BEDROCK_STATE_MAPS[table.mc_version]
        report = java_report(java_version, java, cache, work)
        maps[table.mc_version] = version_families(java_states(report.blocks), mappings, table.blocks)
        print(
            f"bedrock {table.mc_version}: {len(maps[table.mc_version].families)} block pairs "
            f"mapped from Java {java_version}, {len(maps[table.mc_version].refused)} refused",
            flush=True,
        )
    return maps


def state_map_sources(tables: list[Table]) -> list[dict]:
    """Where each Bedrock table's state map came from, as `pack.json` records it."""
    sources = []
    for table in tables:
        commit, sha256, java_version = BEDROCK_STATE_MAPS[table.mc_version]
        url = GEYSER_MAPPINGS.format(commit=commit)
        sources.append({"mc_version": table.mc_version, "url": url, "sha256": sha256, "java": java_version})
    return sources


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


@dataclass(frozen=True)
class Extracted:
    """What one edition's extraction read: its block tables, and Bedrock's state maps."""

    tables: list[Table]
    # Empty for Java, whose states are the canonical vocabulary itself.
    maps: dict[str, VersionMap]


def updated_manifest(manifest: dict, extracted: Extracted) -> dict:
    """`manifest` recording where each block table and state map came from."""
    script = Path(__file__).resolve().relative_to(ROOT).as_posix()
    record = {
        "by": script,
        "blocks": [{"mc_version": t.mc_version, **t.source} for t in extracted.tables],
    }
    if extracted.maps:
        record["states"] = state_map_sources(extracted.tables)
    return {**manifest, "extracted": record}


def pack_files(edition: str, extracted: Extracted) -> dict[Path, str]:
    """The text each extracted pack file should hold."""
    pack = PACKS / edition
    manifest = json.loads((pack / "pack.json").read_text(encoding="utf-8"))
    versions = json.loads((pack / "data_versions.json").read_text(encoding="utf-8"))
    files = {
        pack / manifest["files"]["blocks"]: blocks_text(extracted.tables),
        pack / manifest["files"]["data_versions"]: json_text(
            updated_data_versions(versions, edition, extracted.tables)
        ),
        pack / "pack.json": json_text(updated_manifest(manifest, extracted)),
    }
    if extracted.maps:
        sensitivity = pack / manifest["files"]["sensitivity"]
        try:
            classified = json.loads(sensitivity.read_text(encoding="utf-8"))
        except (OSError, ValueError) as problem:
            raise Refused(f"{sensitivity.relative_to(ROOT)} does not read: {problem}") from None
        if classified.get("schema_version") != SENSITIVITY_SCHEMA:
            raise Refused(f"{sensitivity.relative_to(ROOT)} is not schema {SENSITIVITY_SCHEMA}")
        files[pack / manifest["files"]["states"]] = states_text(extracted.maps, classified)
    return files


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


def extract(edition: str, java: str, cache: Path, work: Path) -> Extracted:
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
    maps = extract_state_maps(tables, java, cache, work) if edition == "bedrock" else {}
    return Extracted(tables, maps)


def main(argv: list[str] | None = None) -> int:
    """0 when the files hold, or now hold, what the game says; 1 on a refusal or, with `--check`, a
    difference; 2 on misuse."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("edition", choices=("java", "bedrock"))
    parser.add_argument(
        "--java",
        default="java",
        help="a Java new enough for every Java target and every Java release a state map follows",
    )
    parser.add_argument(
        "--cache",
        type=Path,
        default=Path.home() / ".cache" / "cairn-minecraft",
        help="where the server jars, palettes and state maps are kept",
    )
    parser.add_argument(
        "--check", action="store_true", help="write nothing; fail when a file differs"
    )
    args = parser.parse_args(argv)
    try:
        with tempfile.TemporaryDirectory(prefix="cairn-extract-registry-") as work:
            java = os.path.normpath(args.java)
            extracted = extract(args.edition, java, args.cache.resolve(), Path(work).resolve())
        files = pack_files(args.edition, extracted)
    except Refused as refusal:
        print(f"error: {refusal}", file=sys.stderr, flush=True)
        return 1
    committed = {path: path.read_text(encoding="utf-8") if path.exists() else "" for path in files}
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
