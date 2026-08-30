//! The actuator and sensor tables in `synth`, held against `core`'s
//! per-role argument vocabularies.
//!
//! `ACTUATOR_BINDINGS` pairs each `spec/redstone` §14.2 argument key with the
//! component that carries it, and `MemberRole::arguments` is the vocabulary
//! `check` refuses against. Where both know the keyword they have to agree,
//! or `check` refuses a source the redstone front end is built to read.
//!
//! Asked from this crate because this is where the pairing lives, and asked
//! against the constants themselves rather than a copy of them — a local
//! restatement would go stale on a change to `synth.rs` without failing
//! here, which is the failure this exists to catch.

use cairn_lang_core::intent::{MemberRole, role_of};
use cairn_lang_redstone::synth::{ACTUATOR_BINDINGS, SENSOR_HOSTS};

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
        "exactly one actuator host is a keyword today; a second one landing \
         should be noticed here rather than silently widening what this test \
         covers",
    );
}

#[test]
fn every_sensor_host_is_a_keyword_the_role_table_knows() {
    // A tail sits on the member, not on an argument, so there is no
    // vocabulary entry to check. What must hold is that the host is a real
    // keyword: `synth` refuses a `->` on anything else, and a host the role
    // table has never heard of would refuse every source that uses it.
    assert!(
        !SENSOR_HOSTS.is_empty(),
        "an empty host list refuses every tail"
    );
    for host in SENSOR_HOSTS {
        assert!(
            role_of(host).arguments().is_some(),
            "`{host}` carries a `->` tail per spec/redstone §14.2 and is not a known keyword",
        );
    }
}
