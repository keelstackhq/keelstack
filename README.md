# keelstack

The keelstackhq tools in one repository (decision D-023).

| Folder | What it is |
| --- | --- |
| `bosun/` | Orchestration helper for repos where agents write all the code |
| `splice/` | Bootstrap a repo from versioned template pieces and keep it current |
| `hull/` | Opinionated, AI-native monorepo template (pnpm, not in the Cargo workspace) |
| `hoist/` | Provision and verify a repo's GitHub and Cloudflare setup |
| `hoard/` | One-line CI cache for TypeScript monorepos |
| `berth/` | An isolated, ready workspace for each parallel coding agent |
| `keel/` | Bootstrap a repo with the keelstackhq tools |
| `decisions/` | Settled decisions that apply across keelstackhq repos |

The Rust crates form one Cargo workspace:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

hull: `pnpm --dir hull install && pnpm --dir hull test`.

`MIGRATION.md` lists the source repo and imported commit of each folder.
