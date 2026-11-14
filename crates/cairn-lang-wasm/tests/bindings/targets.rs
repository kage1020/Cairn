//! `targets`: the versions each edition's built-in pack can build for.

use cairn_lang_core::Edition;
use cairn_lang_wasm::report::{Targets, targets};

fn owned(versions: &[&str]) -> Vec<String> {
    versions.iter().map(|&v| v.to_owned()).collect()
}

#[test]
fn each_edition_lists_its_versions_in_release_order_with_the_one_latest_names() {
    assert_eq!(
        targets(Edition::Java),
        Targets {
            latest: "26.3".to_owned(),
            versions: owned(&["1.20.4", "1.21", "1.21.4", "26.3"]),
        },
    );
    assert_eq!(
        targets(Edition::Bedrock),
        Targets {
            latest: "1.26.60".to_owned(),
            versions: owned(&["1.21.0", "1.21.40", "1.21.60", "1.26.60"]),
        },
    );
}
