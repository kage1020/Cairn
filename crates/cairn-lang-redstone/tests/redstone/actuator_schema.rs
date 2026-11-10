//! Each [`Actuator`] agrees with `core`'s per-role argument vocabularies.

use cairn_lang_core::intent::role_of;
use cairn_lang_redstone::synth::Actuator;

#[test]
fn every_actuator_key_is_in_its_host_vocabulary() {
    for &actuator in Actuator::ALL {
        let (key, host) = (actuator.key(), actuator.keyword());
        let role = role_of(host);
        let vocabulary = role
            .own_keys()
            .unwrap_or_else(|| panic!("`{host}` is no keyword, so `{key}=` has nowhere to sit"));
        assert!(
            vocabulary.contains(&key),
            "`{host}` vocabulary lacks `{key}`",
        );
        assert_eq!(role.actuator(), Some(actuator), "`{host}`");
        assert_eq!(Actuator::reading(key), Some(actuator), "`{key}`");
    }
}
