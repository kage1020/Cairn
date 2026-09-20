//! The actuator table in `synth`, held against `core`'s per-role argument
//! vocabularies.
//!
//! `ACTUATOR_BINDINGS` pairs each argument key from `spec/redstone`
//! "Signal binding" with the component that carries it, and
//! `MemberRole::arguments` is the vocabulary `check` refuses against.
//! Where both know the keyword they have to agree, or `check` refuses a
//! source the redstone front end is built to read.
//!
//! Asked from this crate because this is where the pairing lives, and asked
//! against the constant itself rather than a copy of it — a local
//! restatement would go stale on a change to `synth.rs` without failing
//! here, which is the failure this exists to catch. The sensor half of the
//! pair is `core`'s table now, and its own test sits beside it in
//! `cairn-lang-core/tests/check_binding.rs`.

use cairn_lang_core::intent::{MemberRole, role_of};
use cairn_lang_redstone::synth::ACTUATOR_BINDINGS;

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
            "`{host}` carries `{key}=` per `spec/redstone` \"Signal binding\", and \
             `check` would refuse it",
        );
        checked += 1;
    }
    assert_eq!(
        checked, 1,
        "exactly one actuator host is a keyword today; a second one landing \
         should be noticed here rather than silently widening what this test \
         covers",
    );
}
