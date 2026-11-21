//! The structure id a file is installed under, and the file in the world it names.

use std::path::Path;

use cairn_lang_formats::world::{
    DEFAULT_NAMESPACE, IdPart, Namespace, StructureId, StructureIdError, WorldVersion,
};

fn id(namespace: &str, name: &str) -> Result<StructureId, StructureIdError> {
    StructureId::new(&Namespace::new(namespace)?, name)
}

fn refusal(namespace: &str, name: &str) -> StructureIdError {
    id(namespace, name).expect_err("refused")
}

/// A world the game last saved at `data_version`.
fn saved_at(data_version: i32) -> WorldVersion {
    WorldVersion {
        data_version,
        name: None,
    }
}

#[test]
fn an_id_names_a_file_in_the_folder_its_worlds_version_loads_from() {
    let cottage = id(DEFAULT_NAMESPACE, "cottage").expect("valid");
    assert_eq!(cottage.to_string(), "minecraft:cottage");
    assert_eq!(
        cottage.file_in(Path::new("saves/New World"), &saved_at(4774)),
        Path::new("saves/New World/generated/minecraft/structure/cottage.nbt")
    );
    assert_eq!(
        cottage.file_in(Path::new("saves/New World"), &saved_at(4773)),
        Path::new("saves/New World/generated/minecraft/structures/cottage.nbt")
    );
}

#[test]
fn a_name_holding_folders_names_a_nested_file() {
    let nested = id("my_pack-2.0", "houses/v1.2/oak_cottage").expect("valid");
    assert_eq!(nested.namespace().as_str(), "my_pack-2.0");
    assert_eq!(nested.name(), "houses/v1.2/oak_cottage");
    assert_eq!(nested.to_string(), "my_pack-2.0:houses/v1.2/oak_cottage");
    assert_eq!(
        nested.file_in(Path::new("w"), &saved_at(5023)),
        Path::new("w/generated/my_pack-2.0/structure/houses/v1.2/oak_cottage.nbt")
    );
    let dotted = id("minecraft", "cottage.v2").expect("valid");
    assert_eq!(
        dotted.file_in(Path::new("w"), &saved_at(4189)),
        Path::new("w/generated/minecraft/structures/cottage.v2.nbt")
    );
}

#[test]
fn a_file_name_may_end_in_a_dot_the_game_adds_nbt_after() {
    let dotted = id("minecraft", "houses/cottage.").expect("valid");
    assert_eq!(
        dotted.file_in(Path::new("w"), &saved_at(4189)),
        Path::new("w/generated/minecraft/structures/houses/cottage..nbt")
    );
}

#[test]
fn the_namespace_and_the_name_are_checked_each_on_its_own() {
    assert_eq!(
        IdPart::Namespace.check("My Pack"),
        Err(StructureIdError::Character {
            part: IdPart::Namespace,
            value: "My Pack".to_owned(),
            ch: 'M',
        })
    );
    assert_eq!(
        IdPart::Name.check("Cottage"),
        Err(StructureIdError::Character {
            part: IdPart::Name,
            value: "Cottage".to_owned(),
            ch: 'C',
        })
    );
    assert_eq!(IdPart::Name.check("houses/cottage"), Ok(()));
    assert!(IdPart::Name.allows('/'));
    assert!(!IdPart::Namespace.allows('/'));
    assert!(!IdPart::Name.allows('A'));
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
fn a_folder_ending_in_a_dot() {
    for (namespace, name, part, segment) in [
        ("minecraft", "../cottage", IdPart::Name, ".."),
        ("minecraft", "houses/./cottage", IdPart::Name, "."),
        ("minecraft", "houses./cottage", IdPart::Name, "houses."),
        ("..", "cottage", IdPart::Namespace, ".."),
        ("pack.", "cottage", IdPart::Namespace, "pack."),
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
        id("minecraft", name).expect(name);
    }
}
