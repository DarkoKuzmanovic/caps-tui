//! Where the CLI looks for its config when nothing is set.
//!
//! A `.desktop` launch starts in `$HOME`, so a cwd-relative default would open an empty editor —
//! and an apply from it would delete every live binding. These tests pin the order without
//! touching the environment.

use caps_gui::cli::{default_conf_from, default_install_from};

#[test]
fn a_config_beside_the_binary_wins() {
    let dir = tempfile::tempdir().expect("temp dir");
    let beside = dir.path().join("default.conf");
    std::fs::write(&beside, "[nav]\n").expect("write");
    assert_eq!(
        default_conf_from(Some(dir.path()), None, None),
        std::fs::canonicalize(&beside).expect("canonical")
    );
}

#[test]
fn a_checkout_two_levels_up_is_found() {
    // Cargo's layout: target/debug/caps-gui, so ../../default.conf is the checkout's.
    let dir = tempfile::tempdir().expect("temp dir");
    let checkout = dir.path().join("default.conf");
    std::fs::write(&checkout, "[nav]\n").expect("write");
    let exe_dir = dir.path().join("target/debug");
    std::fs::create_dir_all(&exe_dir).expect("dirs");
    assert_eq!(
        default_conf_from(Some(&exe_dir), None, None),
        std::fs::canonicalize(&checkout).expect("canonical")
    );
}

#[test]
fn the_checkout_the_binary_was_built_from_is_found() {
    let dir = tempfile::tempdir().expect("temp dir");
    let checkout = dir.path().join("default.conf");
    std::fs::write(&checkout, "[nav]\n").expect("write");
    let crate_dir = dir.path().join("caps-gui");
    std::fs::create_dir_all(&crate_dir).expect("dirs");
    assert_eq!(
        default_conf_from(None, Some(&crate_dir), None),
        std::fs::canonicalize(&checkout).expect("canonical")
    );
}

#[test]
fn the_config_directory_is_the_fallback() {
    let dir = tempfile::tempdir().expect("temp dir");
    let config = dir.path().join("caps-gui");
    assert_eq!(
        default_conf_from(None, None, Some(&config)),
        config.join("default.conf")
    );
}

#[test]
fn with_nothing_available_the_working_directory_is_last() {
    assert_eq!(default_conf_from(None, None, None), std::path::PathBuf::from("default.conf"));
    assert_eq!(default_install_from(None), std::path::PathBuf::from("install.sh"));
}

#[test]
fn install_sh_is_found_the_same_way() {
    let dir = tempfile::tempdir().expect("temp dir");
    let script = dir.path().join("install.sh");
    std::fs::write(&script, "#!/bin/sh\n").expect("write");
    assert_eq!(
        default_install_from(Some(dir.path())),
        std::fs::canonicalize(&script).expect("canonical")
    );
}
