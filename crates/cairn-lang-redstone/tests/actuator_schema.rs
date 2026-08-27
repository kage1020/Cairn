//! The one actuator key whose host is a keyword the surface accepts must be
//! in that keyword's argument vocabulary.
//!
//! `ACTUATOR_BINDINGS` in `synth` pairs each `spec/redstone` §14.2 argument
//! key with the component that carries it, and `MemberRole::arguments` in
//! `core` is the vocabulary `check` refuses against. Three of the four
//! hosts — `lamp`, `piston`, `dispenser` — are not keywords yet and so have
//! no role to be listed on; `door` is, and `opened_by=` has to be writable
//! on it or `check` refuses a source the redstone front end is built to
//! read.
//!
//! Asked from this crate because this is where the pairing lives. Merging
//! the two tables is what this replaces: they answer different questions
//! (which component carries a key, versus which keys a role reads) and only
//! overlap where both know the keyword.

use cairn_lang_core::intent::{MemberRole, role_of};

/// `spec/redstone` §14.2's actuator keys and their hosts, as `synth`'s
/// `ACTUATOR_BINDINGS` spells them. Restated rather than imported because
/// the constant is private, which is also what makes this test worth
/// having: the two copies disagreeing is the failure it catches.
const ACTUATOR_BINDINGS: &[(&str, &str)] = &[
    ("opened_by", "door"),
    ("powered_by", "piston"),
    ("lit_by", "lamp"),
    ("fired_by", "dispenser"),
];

#[test]
fn every_actuator_key_with_a_real_host_is_in_that_host_vocabulary() {
    let mut checked = 0;
    for (key, host) in ACTUATOR_BINDINGS {
        let role = role_of(host);
        // A host the surface does not accept has no vocabulary to be in,
        // and `synth` refuses the key wherever it is written.
        let Some(vocabulary) = role.arguments() else {
            assert!(
                matches!(role, MemberRole::Other(_)),
                "`{host}` classified as a role but answered no vocabulary",
            );
            continue;
        };
        assert!(
            vocabulary.contains(key),
            "`{host}` carries `{key}=` per spec/redstone §14.2, and `check` would refuse it",
        );
        checked += 1;
    }
    assert_eq!(
        checked, 1,
        "exactly one actuator host is a keyword today; a second one landing should be \
         noticed here rather than silently widening what this test covers",
    );
}
