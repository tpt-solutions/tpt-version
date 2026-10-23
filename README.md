# tpt-version

A content-addressable, structure-aware version control engine for tpt-model
entities. It gives CAD models, documents and spreadsheets Git-like branching,
merging and history, but it works on entity fields instead of text lines.

A CAD vertex moved from `(0,0,0)` to `(1,1,1)` is a *modified* entity, not a
removal plus an addition. Two users who change different fields of the same
entity merge automatically. Changes to the same field become a stored conflict
for the application to resolve. Nothing is resolved silently.

## Design

- **Pure Rust, `no_std` + `alloc`.** The core runs on WASM and embedded targets.
  `std` enables the file-system store and the system clock. The `s3` feature
  adds an S3-backed store.
- **Content-addressed.** Every blob, entity, tree, commit and conflict is
  stored under the SHA-256 of its canonical bytes. Identical content gives an
  identical hash and is stored once.
- **Merkle DAG.** Commits point at a tree and their parents. Branches are named
  refs to commits.
- **Signed commits.** Ed25519 signatures cover a commit's canonical bytes. They
  are verified on load, and a signature is part of the commit hash.

## Quick start

```rust
use tpt_version::{ActorId, LogicalClock, MemoryStore, Repository};

let mut repo = Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main")?;
repo.put_entity(&vertex)?;               // `vertex: impl tpt_version::Entity`
let first = repo.commit("initial layout", &ActorId::from_bytes([1; 16]))?;
repo.branch("feature", first)?;
```

See [`examples/cad_branch_merge.rs`](examples/cad_branch_merge.rs) for a full
branch, edit, auto-merge, conflict and resolution walkthrough:

```sh
cargo run --example cad_branch_merge
```

## Layout

| Module | Purpose |
| --- | --- |
| `object` | Object model: hashes, trees, commits, conflicts, canonical encoding |
| `store` | `CasStore` / `RefStore` traits; memory, file-system and S3 backends |
| `dag` | Commit loading, ancestors, descendants, merge bases |
| `diff` | Tree and commit diffs, with field-level changes for modified entities |
| `merge` | Three-way merge with field-level auto-merge and conflict objects |
| `repo` | Staged working copy, branches, commits, merges, conflict resolution |
| `sign` | Ed25519 commit signing and verification |
| `uir` | Structured entity values (`UirNode`) and their encoding |

## Building and testing

```sh
cargo test                                   # std build, all tests
cargo test --features s3                     # include the S3 store
cargo check --no-default-features            # no_std build
cargo check --no-default-features --target wasm32-unknown-unknown
```

## Status

Pre-1.0. The object encoding is pinned by a test, so a change that alters
existing hashes must be deliberate. Dependencies are git dependencies on
`tpt-solutions/tpt-crypto`, so the crate is not yet published.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at
your option.
