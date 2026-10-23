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
use std::collections::BTreeMap;

use tpt_version::error::ObjectError;
use tpt_version::prelude::*;

/// A CAD vertex. Its id is stored in the entity, so it reads back the same.
struct Vertex {
    id: EntityId,
    position: [f64; 3],
}

impl Entity for Vertex {
    fn id(&self) -> EntityId {
        self.id
    }

    fn to_uir(&self) -> UirNode {
        let [x, y, z] = self.position;
        UirNode::Map(BTreeMap::from([
            ("id".into(), UirNode::Scalar(Scalar::Bytes(self.id.as_bytes().to_vec()))),
            ("x".into(), UirNode::from(x)),
            ("y".into(), UirNode::from(y)),
            ("z".into(), UirNode::from(z)),
        ]))
    }

    fn from_uir(node: &UirNode) -> Result<Self, ObjectError> {
        let UirNode::Map(fields) = node else {
            return Err(ObjectError::InvalidValue("vertex"));
        };
        let float = |name| match fields.get(name) {
            Some(UirNode::Scalar(Scalar::Float(value))) => Ok(*value),
            _ => Err(ObjectError::InvalidValue("coordinate")),
        };
        let id = match fields.get("id") {
            Some(UirNode::Scalar(Scalar::Bytes(bytes))) => <[u8; 16]>::try_from(bytes.as_slice())
                .map(EntityId::from_bytes)
                .map_err(|_| ObjectError::InvalidValue("vertex id"))?,
            _ => return Err(ObjectError::InvalidValue("vertex id")),
        };
        Ok(Self {
            id,
            position: [float("x")?, float("y")?, float("z")?],
        })
    }
}

fn main() -> Result<(), RepoError> {
    let author = ActorId::from_bytes([1; 16]);
    let mut repo = Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main")?;

    repo.put_entity(&Vertex {
        id: EntityId::from_bytes([7; 16]),
        position: [0.0, 0.0, 0.0],
    })?;
    let first = repo.commit("initial layout", &author)?;
    repo.branch("feature", first)?;
    Ok(())
}
```

## Examples

Each example is a complete program. Run one with `cargo run --example <name>`.

| Example | Shows |
| --- | --- |
| [`cad_branch_merge`](examples/cad_branch_merge.rs) | Branch, edit, auto-merge, conflict and resolution on CAD vertices |
| [`spreadsheet_cells`](examples/spreadsheet_cells.rs) | Cells keyed by address: clean merges, and a real conflict on one cell |
| [`document_blocks`](examples/document_blocks.rs) | Blocks with a list field, and why a list conflicts as one value |
| [`file_store_cli`](examples/file_store_cli.rs) | A persistent history on disk, driven from the command line |
| [`signed_history`](examples/signed_history.rs) | Ed25519-signed commits, and a forged commit being refused on load |
| [`app_template`](examples/app_template.rs) | An app skeleton: a derived entity, a workspace type, and a merge UI trait |
| [`s3_sync`](examples/s3_sync.rs) | Two machines syncing through a bucket, with a stale push refused (`--features s3`) |

## Documentation

- [Concepts](docs/concepts.md): objects, commits, merging, signing, stores, `no_std`
- [Conflict cookbook](docs/conflicts.md): each kind of conflict and how to settle it
- [Porting your data model](docs/porting.md): granularity, stable ids, lists, floats
- [FAQ](docs/faq.md): how this compares with Git and CRDTs

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

## Command-line tool

`tpt` versions a key-value document kept in a directory. Each command is one
step, and the current branch is remembered in the store.

```sh
cargo run --bin tpt -- commit "first draft" title=Spec owner=ana
cargo run --bin tpt -- branch review
cargo run --bin tpt -- switch review
cargo run --bin tpt -- commit "tighten title" title="Spec v2"
cargo run --bin tpt -- switch main
cargo run --bin tpt -- merge review theirs "take review's title"
cargo run --bin tpt -- log
```

Run `tpt` with no arguments for the full command list. The store defaults to
`.tpt`, and `--dir` picks another.
