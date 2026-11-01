//! `synth`'s `ACTUATOR_BINDINGS` agrees with `core`'s per-role argument vocabularies.

use cairn_lang_core::intent::{MemberRole, role_of};
use cairn_lang_redstone::synth::ACTUATOR_BINDINGS;

#[test]
fn every_actuator_key_with_a_real_host_is_in_that_host_vocabulary() {
    let mut checked = 0;
    for (key, host) in ACTUATOR_BINDINGS {
        let role = role_of(host);
        let Some(vocabulary) = role.arguments() else {
            assert!(
                matches!(role, MemberRole::Other(_)),
                "`{host}` classified as a role but answered no vocabulary",
            );
            continue;
        };
        assert!(
            vocabulary.contains(key),
            "`{host}` vocabulary lacks `{key}`",
        );
        checked += 1;
    }
    assert_eq!(
        checked, 1,
        "expected exactly one actuator host with a vocabulary",
    );
}
