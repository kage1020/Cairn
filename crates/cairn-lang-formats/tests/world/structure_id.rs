//! The structure id a file is installed under, and the file in the world it names.

use std::path::Path;

use cairn_lang_formats::world::{DEFAULT_NAMESPACE, IdPart, StructureId, StructureIdError};

fn refusal(namespace: &str, name: &str) -> StructureIdError {
    StructureId::new(namespace, name).expect_err("refused")
}

#[test]
fn an_id_names_a_file_under_the_worlds_generated_structures() {
    let id = StructureId::new(DEFAULT_NAMESPACE, "cottage").expect("valid");
    assert_eq!(id.to_string(), "minecraft:cottage");
    assert_eq!(
        id.file_in(Path::new("saves/New World")),
        Path::new("saves/New World/generated/minecraft/structures/cottage.nbt")
    );
}

#[test]
fn a_name_holding_folders_names_a_nested_file() {
    let id = StructureId::new("my_pack-2.0", "houses/v1.2/oak_cottage").expect("valid");
    assert_eq!(id.namespace(), "my_pack-2.0");
    assert_eq!(id.name(), "houses/v1.2/oak_cottage");
    assert_eq!(id.to_string(), "my_pack-2.0:houses/v1.2/oak_cottage");
    assert_eq!(
        id.file_in(Path::new("w")),
        Path::new("w/generated/my_pack-2.0/structures/houses/v1.2/oak_cottage.nbt")
    );
    let dotted = StructureId::new("minecraft", "cottage.v2").expect("valid");
    assert_eq!(
        dotted.file_in(Path::new("w")),
        Path::new("w/generated/minecraft/structures/cottage.v2.nbt")
    );
}

#[test]
fn an_empty_part() {
    assert_eq!(
        refusal("", "cottage"),
        StructureIdError::Empty {
            part: IdPart::Namespace
        }
    );
    assert_eq!(
        refusal("minecraft", ""),
        StructureIdError::Empty { part: IdPart::Name }
    );
}

#[test]
fn a_character_the_game_does_not_read() {
    for (namespace, name, part, value, ch) in [
        ("minecraft", "Cottage", IdPart::Name, "Cottage", 'C'),
        ("minecraft", "oak cottage", IdPart::Name, "oak cottage", ' '),
        ("minecraft", "ns:cottage", IdPart::Name, "ns:cottage", ':'),
        ("minecraft", "café", IdPart::Name, "café", 'é'),
        ("my/pack", "cottage", IdPart::Namespace, "my/pack", '/'),
    ] {
        assert_eq!(
            refusal(namespace, name),
            StructureIdError::Character {
                part,
                value: value.to_owned(),
                ch,
            },
            "{namespace}:{name}"
        );
    }
}

#[test]
fn a_name_with_an_empty_segment() {
    for name in ["/cottage", "cottage/", "houses//cottage"] {
        assert_eq!(
            refusal("minecraft", name),
            StructureIdError::EmptySegment {
                name: name.to_owned()
            }
        );
    }
}

#[test]
fn a_segment_ending_in_a_dot() {
    for (namespace, name, part, segment) in [
        ("minecraft", "..", IdPart::Name, ".."),
        ("minecraft", "houses/./cottage", IdPart::Name, "."),
        ("minecraft", "cottage.", IdPart::Name, "cottage."),
        ("..", "cottage", IdPart::Namespace, ".."),
    ] {
        assert_eq!(
            refusal(namespace, name),
            StructureIdError::TrailingDot {
                part,
                segment: segment.to_owned(),
            },
            "{namespace}:{name}"
        );
    }
}

#[test]
fn a_device_name_with_or_without_an_extension() {
    for (namespace, name, part, segment) in [
        ("minecraft", "con", IdPart::Name, "con"),
        ("minecraft", "houses/aux.v2", IdPart::Name, "aux.v2"),
        ("minecraft", "com", IdPart::Name, "com"),
        ("minecraft", "lpt9", IdPart::Name, "lpt9"),
        ("nul", "cottage", IdPart::Namespace, "nul"),
    ] {
        assert_eq!(
            refusal(namespace, name),
            StructureIdError::DeviceName {
                part,
                segment: segment.to_owned(),
            },
            "{namespace}:{name}"
        );
    }
    for name in ["com0", "com10", "console", "lpt", "auxiliary"] {
        StructureId::new("minecraft", name).expect(name);
    }
}
