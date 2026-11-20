# Migration to one monorepo

Decision D-023 (`decisions/023-one-monorepo.md`) moves every keelstackhq repo into this one.
Each repo was imported with its full history into its own folder (`git filter-repo
--to-subdirectory-filter <folder>`, then `git merge --allow-unrelated-histories`). Tags were not
imported.

The final re-sync merges each source repo's commits after the imported SHA below.

| Folder | Source repo | Imported SHA (main) |
| --- | --- | --- |
| bosun | https://github.com/keelstackhq/bosun | b8029e16de95a5883cc451f7e9266c024c7690ae |
| splice | https://github.com/keelstackhq/splice | 1c8b33b751bfdb242daccc7fe26eb7d9e70afb47 |
| hull | https://github.com/keelstackhq/hull | baab2ff26a0943f5be034a78014bc4a5ed40a3fa |
| hoist | https://github.com/keelstackhq/hoist | cb200b5369ac0ef55bdc6a47ebe7f2553be6e82b |
| hoard | https://github.com/keelstackhq/hoard | 3b4b96bca151c241050b06752d407d243cd3593a |
| berth | https://github.com/keelstackhq/berth | 5d1db8191bf8e46937f18a30a2e70a3be7e4a734 |
| keel | https://github.com/keelstackhq/keel | e34f177359f889b2482814f5b611ec90c8fe9339 |
| decisions | https://github.com/keelstackhq/decisions | 5b05c178f0a27d93e06d93089afb330e97bb1243 |

## Changes made by the move

- One Cargo workspace at the root (`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`,
  `deny.toml`, `.cargo/config.toml`); the per-folder copies were deleted.
- All Rust crates share one lockstep version (`[workspace.package] version = "0.21.0"`, the
  highest tool version at the move). The npm launcher packages follow it. Nothing was released.
- Release profiles moved to the root (cargo ignores member profiles).
- splice no longer needs serde_json `preserve_order`: workspace feature unification would have
  turned it on for every tool and changed their JSON key order. splice keeps key order with its
  own ordered value type (`splice/src/merge/value.rs`).
- Per-folder `.github/` directories are kept but do not run here; root CI replaces them.
