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
- `cad_branch_merge` example.

### Design decisions

- `tpt-primitives` is not `no_std` and has no `EntityId` or `ActorId`, so
  identities are local 16-byte values.
- `tpt-uir-core` has no generic value tree, so `UirNode` is defined in this
  crate. Its encoding is pinned by a test.
- Pending merge conflicts are held in memory. A repository reopened mid-merge
  returns to its committed head rather than resuming the merge.
