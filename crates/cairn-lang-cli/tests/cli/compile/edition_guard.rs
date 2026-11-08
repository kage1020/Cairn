//! `@edition` guards end to end: a pinned build writes its own edition's guarded blocks, and checks
//! their ids at the pinned version (`spec/versioning-editions` "An `@edition` guard holds the escape
//! hatch").

use std::fs;

use crate::common::{Fixture, assert_stream, bytes_contain, compile_as, nbt_body, stderr_of};

/// The light block under a guard per edition, inside walls tall enough to hold it.
const LIGHT: &str = "theme t:\n  slot floor -> @oak_planks\nstruct s size=3x3\n  \
                     floor mat_slot=floor\n  walls class=outer mat_slot=floor height=3\n  \
                     @edition java\n    raw_block mat=minecraft:light[level=15] at=1,1,1\n  \
                     @edition bedrock\n    raw_block mat=minecraft:light_block_15 at=1,1,1\n";

/// How many times `needle` occurs in `hay`.
fn occurrences(hay: &[u8], needle: &str) -> usize {
    hay.windows(needle.len())
        .filter(|w| *w == needle.as_bytes())
        .count()
}

#[test]
fn a_pinned_build_writes_its_own_editions_guarded_block_and_not_the_others() {
    let fixture = Fixture::new(&format!("@requires bedrock version>=1.21.40\n{LIGHT}"));

    let java = compile_as(&fixture, "java", "1.21.4");
    assert_eq!(java.status.code(), Some(0), "stderr={}", stderr_of(&java));
    assert_stream(&stderr_of(&java), &[]);
    let nbt = nbt_body(&fixture.out().join("s.nbt"));
    assert!(bytes_contain(&nbt, "minecraft:light"));
    assert!(!bytes_contain(&nbt, "light_block_15"));

    let bedrock = compile_as(&fixture, "bedrock", "1.21.60");
    assert_eq!(
        bedrock.status.code(),
        Some(0),
        "stderr={}",
        stderr_of(&bedrock)
    );
    assert_stream(&stderr_of(&bedrock), &[]);
    let structure = fs::read(fixture.out().join("s.mcstructure")).expect("read artifact");
    assert_eq!(occurrences(&structure, "minecraft:light_block_15"), 1);
    // Every `minecraft:light` is the start of the Bedrock spelling, so the Java one is absent.
    assert_eq!(occurrences(&structure, "minecraft:light"), 1);
}

/// Only the Bedrock guard's id is checked against Bedrock 1.21.0, which spells it `light_block`.
#[test]
fn a_guarded_id_is_checked_at_the_pinned_version_and_the_other_editions_is_not() {
    let fixture = Fixture::new(LIGHT);
    let refused = compile_as(&fixture, "bedrock", "1.21.0");
    let stderr = stderr_of(&refused);
    assert_eq!(refused.status.code(), Some(1), "stderr={stderr}");
    assert_stream(&stderr, &[(9, 19, "error", "E_UNKNOWN_ID")]);
    assert_eq!(fixture.artifacts(), Vec::<String>::new());
}
