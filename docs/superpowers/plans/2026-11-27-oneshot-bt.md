# BT: setup script installs the pinned tools (#133)

Done when UC-BT-1: a fresh cloud session of a rust repo runs every CI step without installing anything.
touch: bosun/channel/pieces/core/files/docs/setup-script.sh.jinja, bosun/tests/setup_script_tools.rs

Gap in the template: cargo-nextest installs only if bosun <0.12.0 or a profile names it; both tools fall
back to `cargo install` (minutes) when a pin/checksum is missing or the download fails.
1. Test first: render/run the script with fake curl/tar/sha256 (see tests/cli_doctor_setup_script.rs, cli_setup_snippet.rs for the harness): rust repo installs cargo-deny AND cargo-nextest from checksummed release tarballs at pins.toml versions; no `cargo install` is ever invoked; checksum mismatch or missing pin => warn, skip, exit 0; non-rust repo installs neither.
2. Edit the template: nextest unconditional with HAS_RUST; drop the cargo install fallback.
3. Run bosun tests; splice/hull/keel render the channel: run their tests if the channel is rendered there.
4. Reviewer.
