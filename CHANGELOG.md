# Changelog

## Unreleased

### Added

- Content-addressable object model: blobs, entities, trees, commits and
  conflicts, each with a canonical encoding and a SHA-256 address.
- `CasStore` and `RefStore` traits, with `MemoryStore`, `FileSystemStore`
  (atomic writes, `std`) and `S3Store` (`s3` feature, client injected through
  `ObjectClient`).
- Commit DAG queries: ancestors, descendants, merge base (deterministic for
  criss-cross histories) and commit signature verification on load.
- Structural diffs: added, removed and modified entities, with field-level
  changes at each path.
- Three-way merge with field-level auto-merge. Delete-versus-modify and
  overlapping field edits are stored as conflicts. Fast-forward merges.
- `Repository`: staged working copy, commits, branches, detached checkout,
  merge, and `resolve_conflict` with `Ours`, `Theirs` or a custom value. Commits
  are refused while conflicts remain.
- Ed25519 commit signing through the injected `Signer` trait. Signature checks
  reject small-order keys and commitments.
- `no_std` + `alloc` core; WASM and Cortex-M targets check cleanly.
- `cad_branch_merge` example, plus `spreadsheet_cells`, `document_blocks`,
  `file_store_cli`, `signed_history` and `s3_sync` examples.
- `prelude` module. The README Quick Start is compiled as a doctest.
- `docs/`: concepts, conflict cookbook, porting guide and FAQ.

### Changed

- `RefStore::compare_and_swap_ref`. `commit`, `merge` and `branch` use it, so a
  stale repository gets `RepoError::RefMoved` instead of overwriting another
  writer. The file-system store locks each ref with a `.lock` file while it
  swaps. The S3 store uses ETags through the new `ConditionalClient` trait.
- `CasStore::contains` returns `Result<bool, StoreError>`, so a backend failure
  is no longer reported as a missing object.
- `S3Store` implements `RefStore`, so branches can live in S3.
- Floats compare by bit pattern, so an unchanged `NaN` is not a change.
- `put_entity` refuses an entity whose nesting exceeds the decode limit, or
  whose stored form loads under a different id (`RepoError::IdMismatch`).
- Branch names reject a `.lock` suffix, Windows device names, and case-only or
  path-prefix clashes (`RepoError::BranchNameClash`).
- Merge-base search loads each commit once. It was quadratic in the number of
  common ancestors.
- Error enums are `#[non_exhaustive]`.

### Fixed

- File-system writes use unique temp files and `sync_all` before the rename, so
  concurrent writers do not share a temp file or publish unsynced contents.

### Design decisions

- `tpt-primitives` is not `no_std` and has no `EntityId` or `ActorId`, so
  identities are local 16-byte values.
- `tpt-uir-core` has no generic value tree, so `UirNode` is defined in this
  crate. Its encoding is pinned by a test.
- Pending merge conflicts are held in memory. A repository reopened mid-merge
  returns to its committed head rather than resuming the merge.
