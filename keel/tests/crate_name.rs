//! The crate is published as `keelstack-cli` (`keel-cli` on crates.io is another project's);
//! the binary, the CLI and the npm package stay `keel`.

#[test]
fn the_crate_is_keelstack_cli_and_its_binary_is_keel() {
    assert_eq!(env!("CARGO_PKG_NAME"), "keelstack-cli");
    let bin = std::path::Path::new(env!("CARGO_BIN_EXE_keel"));
    let stem = bin.file_stem().and_then(|s| s.to_str());
    assert_eq!(stem, Some("keel"));
    assert!(bin.exists(), "{}", bin.display());
}
