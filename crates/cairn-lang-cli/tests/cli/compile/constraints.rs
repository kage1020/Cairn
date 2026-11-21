//! A pinned `cairn compile` runs the constraint catalog's checks, warns on what the game would
//! drop, and still writes the build (`spec/lint` "Constraint checks").

use crate::common::{Fixture, Run, assert_stream, compile_as};

#[test]
fn a_pinned_compile_warns_on_a_lever_on_a_pane_and_writes_the_build() {
    let fixture = Fixture::new(concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n\n",
        "struct s size=5x5\n",
        "  floor mat_slot=wall\n",
        "  walls mat_slot=wall height=3\n",
        "  raw_block mat=minecraft:glass_pane at=2,1,2\n",
        "  raw_block mat=minecraft:lever[face=wall, facing=south] at=2,1,3\n",
    ));
    let run = Run::from(compile_as(&fixture, "java", "1.21.4"));
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_stream(&run.stderr, &[(8, 3, "warning", "W_NO_ATTACHMENT_FACE")]);
    assert_eq!(fixture.artifacts(), ["s.crn.lock", "s.nbt"]);
}
