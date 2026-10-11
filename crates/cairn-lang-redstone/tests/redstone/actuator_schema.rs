//! The actuator table in `synth` agrees with `core`'s per-role argument vocabularies:
//! `ACTUATOR_BINDINGS` pairs each key from `spec/redstone` "Signal binding" with its component,
//! and where `MemberRole::arguments` knows the keyword too the two must agree, or `check` refuses
//! a source this front end is built to read. Asked of the constant itself, since a copy would go
//! stale without failing. The sensor half is tested in `cairn-lang-core/tests/check/binding.rs`.

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
