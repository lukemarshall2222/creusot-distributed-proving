// Feature: creusot-distributed-proving, Property 5: Invalid configurations and pin mismatches fail closed
// P-ID: P1.1.6, P2.1.3

//! The `dp.toml` format over generated texts: a text of the format reads
//! back as what was written, and a text with an option that the format
//! does not have, a value of the wrong type or an impossible time is
//! refused with a message that names it.

use dp_formats::dptoml::ProjectConfig;
use dp_testkit::gen::config_text::{print, project_config, refused_text};
use proptest::prelude::*;

proptest! {
    #![proptest_config(dp_testkit::config::cases("prop_config", 500))]

    #[test]
    fn a_text_of_the_format_reads_back(config in project_config()) {
        let text = print(&config);
        let parsed = ProjectConfig::parse(&text);
        prop_assert_eq!(parsed.as_ref(), Ok(&config), "{}", text);
    }

    #[test]
    fn a_fault_is_refused_and_named(refused in refused_text()) {
        match ProjectConfig::parse(&refused.text) {
            Ok(config) => prop_assert!(false, "accepted {:?} as {:?}", refused.text, config),
            Err(error) => prop_assert!(error.0.contains(&refused.names), "{:?} does not name {:?}", error.0, refused.names),
        }
    }

    // Reading is total: no text makes it panic.
    #[test]
    fn no_text_makes_the_reader_panic(text in "\\PC{0,200}") {
        let _ = ProjectConfig::parse(&text);
    }
}
