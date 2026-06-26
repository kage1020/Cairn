//! `BlockArray` → Java vanilla structure NBT tag tree.
//!
//! The Java structure file format ships a root [`Compound`] with:
//!
//! ```text
//! size:        List<Int>[3]
//! palette:     List<Compound>  (each: { Name: String, Properties: Compound? })
//! blocks:      List<Compound>  (each: { state: Int, pos: List<Int>[3] })
//! entities:    List<empty>
//! DataVersion: Int
//! ```
//!
//! `block_entities` and `entities` from the IR are intentionally not wired
//! through — the M2 lowering passes never populate them, and the Java
//! vanilla structure format expects them either absent or empty. Keeping
//! that decision local to this module (rather than failing if the lists
//! aren't empty) leaves the door open for M3 fixtures lowering.

use cairn_lang_core::block_array::{BlockArray, BlockState};
pub use cairn_lang_nbt::Compound;
use cairn_lang_nbt::tag::{List, Tag};
use cairn_lang_nbt::{NbtIoError, write_java_gzip};
use thiserror::Error;

use crate::data_version::JavaTarget;

/// Errors raised while serialising a [`BlockArray`] to a Java structure.
#[derive(Debug, Error)]
pub enum JavaStructureError {
    /// Forwarded I/O / encoding failure from the NBT writer.
    #[error("nbt: {0}")]
    Nbt(#[from] NbtIoError),
    /// A palette entry's `id` lacks a `namespace:identifier` form. Abstract
    /// material tokens (`@cobblestone`) are expected to be resolved to a
    /// concrete `minecraft:cobblestone` before reaching the backend; an
    /// unresolved one is a build invariant violation, not a recoverable
    /// state.
    #[error("palette entry `{id}` is abstract; expected `namespace:identifier`")]
    AbstractPaletteEntry {
        /// Offending id verbatim.
        id: String,
    },
    /// A voxel dimension overflowed the `i32` wire width Java NBT uses.
    /// In practice this can only fire on a structure bigger than `i32::MAX`
    /// blocks along one axis — far beyond what `cairn lower` or any sane
    /// build descriptor produces — but failing loud beats writing a
    /// truncated header.
    #[error("dimension {axis} = {value} exceeds Java NBT i32 wire limit")]
    DimensionOverflow {
        /// Which axis overflowed (`"x"` / `"y"` / `"z"`).
        axis: &'static str,
        /// Offending dimension value.
        value: u32,
    },
}

/// Build the root [`Compound`] for a Java vanilla structure file from a
/// lowered [`BlockArray`].
///
/// Pure: no I/O happens here, so the same tree can be serialised twice (for
/// the `resolved_ir_hash` and for the on-disk artifact) without rebuilding.
///
/// # Errors
///
/// Returns [`JavaStructureError::AbstractPaletteEntry`] when a palette
/// entry's id has no `:` separator — the registry pack normally resolves
/// abstract tokens before they reach this point, but a `cairn lower` run
/// without a pack can leak them in and we refuse to write a malformed
/// structure rather than emit one Minecraft will silently treat as air.
pub fn build_structure_tag(
    ba: &BlockArray,
    target: &JavaTarget,
) -> Result<Compound, JavaStructureError> {
    for entry in &ba.palette.entries {
        if !is_concrete_id(&entry.id) {
            return Err(JavaStructureError::AbstractPaletteEntry {
                id: entry.id.clone(),
            });
        }
    }

    let size_x = dim_to_i32(ba.dims.x, "x")?;
    let size_y = dim_to_i32(ba.dims.y, "y")?;
    let size_z = dim_to_i32(ba.dims.z, "z")?;

    let mut root = Compound::new();
    root.insert("size", Tag::List(List::of_ints([size_x, size_y, size_z])));
    root.insert("palette", Tag::List(palette_list(&ba.palette.entries)));
    root.insert("blocks", Tag::List(blocks_list(ba)?));
    root.insert("entities", Tag::List(List::empty()));
    root.insert("DataVersion", Tag::Int(target.data_version));
    Ok(root)
}

fn dim_to_i32(value: u32, axis: &'static str) -> Result<i32, JavaStructureError> {
    i32::try_from(value).map_err(|_| JavaStructureError::DimensionOverflow { axis, value })
}

/// Build + gzip-write a [`BlockArray`] to the given writer in one step.
///
/// # Errors
///
/// Forwards every failure mode of [`build_structure_tag`] and any I/O the
/// gzip encoder raises.
pub fn write_structure_gzip<W: std::io::Write>(
    writer: &mut W,
    ba: &BlockArray,
    target: &JavaTarget,
) -> Result<(), JavaStructureError> {
    let root = build_structure_tag(ba, target)?;
    write_compound_gzip(writer, &root)?;
    Ok(())
}

/// Gzip-write an already-built structure [`Compound`] under the empty root
/// name vanilla expects. Split out from [`write_structure_gzip`] so a
/// caller can build every tree first and only then start touching the
/// filesystem (the CLI relies on this to validate the IR before writing
/// any `.nbt`).
///
/// # Errors
///
/// Propagates I/O failure from `writer` and any encoding error raised by
/// the tag tree.
pub fn write_compound_gzip<W: std::io::Write>(
    writer: &mut W,
    root: &Compound,
) -> Result<(), NbtIoError> {
    write_java_gzip(writer, "", root)
}

/// Filename for a single [`BlockArray`] within a multi-structure IR.
///
/// Strips the source-scope prefix:
/// - `"struct::cottage"` → `"cottage.nbt"`
/// - `"site::hamlet::home1"` → `"home1.nbt"` — per-`place` placements
///   share an output directory with sibling structs; the site name is
///   collision-avoided inside the IR key, not on disk.
/// - `"walkway::hamlet::home1.entry__home2.entry"` →
///   `"hamlet_walkway_home1_entry__home2_entry.nbt"` — the site name
///   is preserved so a multi-site file's walkways do not collide on
///   disk, and the `.` separators between place and port id are
///   flattened to `_` so the on-disk name stays a single identifier
///   token across operating systems.
///
/// Kept here (not in the CLI) so the wasm playground and any other consumer
/// agree on naming when they land.
#[must_use]
pub fn output_filename(source_scope: &str) -> String {
    if let Some(rest) = source_scope.strip_prefix("walkway::")
        && let Some((site, ports)) = rest.split_once("::")
    {
        // Flatten `place.port` separators to `_` for portable filenames.
        // Cairn ids reject `.`, so the only path that could fold two
        // distinct scopes into one filename — `a.b__c.d` colliding with
        // `a_b__c_d` — cannot be produced by a syntactically valid
        // module. If id grammar later allows `.` (or `_` becomes a
        // separator), revisit this flatten before the on-disk shape
        // becomes ambiguous.
        let flattened = ports.replace('.', "_");
        return format!("{site}_walkway_{flattened}.nbt");
    }
    let bare = source_scope
        .strip_prefix("struct::")
        .or_else(|| {
            source_scope
                .strip_prefix("site::")
                .and_then(|rest| rest.split_once("::").map(|(_, id)| id))
        })
        .unwrap_or(source_scope);
    format!("{bare}.nbt")
}

fn is_concrete_id(id: &str) -> bool {
    // A concrete Minecraft id always carries exactly one `:` separating a
    // non-empty namespace from a non-empty path. Abstract tokens lowered
    // from `@cobblestone` look like `@cobblestone` — no colon at all — and
    // a stray empty side (`:foo` / `foo:`) is treated as abstract too so a
    // typo never reaches the NBT layer silently.
    let mut parts = id.splitn(2, ':');
    let ns = parts.next().unwrap_or("");
    let path = parts.next();
    matches!(path, Some(p) if !ns.is_empty() && !p.is_empty())
}

fn palette_list(entries: &[BlockState]) -> List {
    let compounds: Vec<Compound> = entries.iter().map(palette_entry).collect();
    List::of_compounds(compounds)
}

fn palette_entry(state: &BlockState) -> Compound {
    let mut c = Compound::new();
    c.insert("Name", Tag::String(state.id.clone()));
    if !state.properties.is_empty() {
        let mut props = Compound::new();
        for (k, v) in &state.properties {
            props.insert(k.clone(), Tag::String(v.clone()));
        }
        c.insert("Properties", Tag::Compound(props));
    }
    c
}

fn blocks_list(ba: &BlockArray) -> Result<List, JavaStructureError> {
    // The IR stores voxels in (y, z, x) order; emit blocks in the same
    // order so successive compiles produce byte-identical output. AIR cells
    // are included — vanilla mojang structure block output keeps them, and
    // M3 site placement needs them to distinguish "void" from "explicit
    // air" cells.
    //
    // The per-axis bounds were already validated by `dim_to_i32` in the
    // caller, so the per-voxel `try_from` here only re-fires when `dims`
    // itself was already over the wire limit, which we've rejected. We
    // still surface a `DimensionOverflow` rather than `expect` so a future
    // direct caller of `blocks_list` cannot panic.
    let mut entries: Vec<Compound> = Vec::with_capacity(ba.dims.volume());
    for y in 0..ba.dims.y {
        let yi = dim_to_i32(y, "y")?;
        for z in 0..ba.dims.z {
            let zi = dim_to_i32(z, "z")?;
            for x in 0..ba.dims.x {
                let xi = dim_to_i32(x, "x")?;
                let i = ba
                    .dims
                    .index(x, y, z)
                    .expect("voxel coordinate in dims by construction");
                let palette_index = ba.voxels[i];
                let mut entry = Compound::new();
                entry.insert("state", Tag::Int(i32::from(palette_index.0)));
                entry.insert("pos", Tag::List(List::of_ints([xi, yi, zi])));
                entries.push(entry);
            }
        }
    }
    Ok(List::of_compounds(entries))
}
