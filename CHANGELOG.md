# Changelog

## 1.0.0 (prepared, not published)

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
- `sync::push` and `sync::fetch` copy reachable objects between any two stores,
  and move the branch only fast-forward.
- `Repository::entity_at` reads an entity from a past commit, and
  `commit_as_of` / `entity_as_of` find the state at a timestamp.
- `export::diff_to_json`, `export::node_json` and `export::diff_to_html`.
- `app_template` example: a derived entity, a workspace type, and a merge UI trait.
- `MergePolicy` and `FieldStrategy` (`Sum`, `Max`, `Min`): numeric fields changed
  on both sides merge by strategy instead of conflicting. `ListStrategy::Union`
  treats a list as a set, and a removal on either side wins.
- `ListStrategy::Ordered`: a sequence-aware three-way merge over list items.
- `oplog`: causal delivery (`Replica::deliver` holds operations until their
  dependencies arrive) and storage (`OpLog::save` / `load`, as `Object::OpLog`).
- `oplog`: a live-collaboration op log. Replicas stamp edits with Lamport clocks,
  and logs converge in any merge order. It sits beside the DAG, not inside it.
- Signed tags: `Repository::create_annotated_tag` and `annotated_tag`, stored as
  `Object::AnnotatedTag` under `tags/`. `resolve_tag` accepts both kinds.
- Large snapshots (over 256 entities) are stored as shards under `Object::TreeRoot`.
  Small snapshots keep their flat encoding and hashes. `Repository::head_tree`
  reads either layout.
- Criss-cross merges use a virtual base built from every best common ancestor
  (`dag::merge_bases`), instead of one arbitrary base.
- `wasm/`: a wasm-bindgen crate and a static demo page (not yet run in a browser).
- Fixed: maps and trees decode only with strictly ascending keys. Before, an
  out-of-order encoding of the same value was accepted, and it hashed differently.
- `tpt` command-line tool (`src/bin/tpt.rs`, default `cli` feature), with `init` and `diff` added.
- Signed reviews: `Object::Review`, with `Repository::review` / `reviews_of`. They
  are stored under `reviews/` refs, which branch names may not use.
- `FieldStrategy::LastWriterWins`: a branch merge takes the value from the newer
  commit (by timestamp, then hash).
- `FieldStrategy::TextLines`: a line-based three-way text merge. Edits to the
  same or adjacent lines still conflict.
- `#[derive(Entity)]` (default `derive` feature). Field types implement the new
  `UirValue` trait, which covers the scalars, `EntityId`, `Option`, `Vec` and arrays.
- History editing: `amend`, `reset_hard`, `revert` and `cherry_pick`. Revert and
  cherry-pick stage a three-way merge, so clashes surface as conflicts.
- Tags: `create_tag`, `resolve_tag`, `list_tags`, `delete_tag`, under `tags/`.
  Branch names may not start with `tags/`. `Repository::status`.
- `Repository::entity_history` and `Repository::field_blame`, which follow the
  first-parent chain.
- `TrustPolicy`: trusted signing keys, and branches that must be signed by one.
  Enforced on commit and on merge into a governed branch.
- `Repository::diff_working` and the `Hooks` trait (`pre_commit` can refuse a
  commit, `post_merge` runs after a merge).
- `maintenance::fsck` reports missing, corrupt and badly signed objects, and
  broken refs. `Repository::gc` removes unreachable objects, via `PruneStore`.
- Pending merges survive between processes. A merge that leaves conflicts is
  saved under `merges/<branch>/` refs, and `Repository::open` resumes it. Those
  refs are gc roots, and branch names may not use the `merges/` prefix.
- `Validator` and `Repository::with_validator`. A validator sees the whole
  snapshot, so it can check rules that span entities. Commits are refused while
  one reports a problem (`RepoError::ValidationFailed`). `Repository::validate`
  runs them on demand.
- `tpt conflicts`, `tpt resolve <key> ours|theirs` and `tpt finish-merge`, which
  settle a conflicted merge over several runs. `tpt merge <branch> <message>`
  leaves conflicts pending, where `merge <branch> ours|theirs <message>` settles
  them all at once.

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
- Benchmarks use Criterion (`cargo bench --bench throughput`). CI compiles them.
- `tpt merge` that fast-forwards, or is already up to date, no longer makes an
  empty commit on top of the result.

### Fixed

- File-system writes use unique temp files and `sync_all` before the rename, so
  concurrent writers do not share a temp file or publish unsynced contents.
- `Repository::merge_review`: while a merge is pending, its result against each side.
- Cherry-pick follows commit order for `LastWriterWins`, as merges do. Before,
  the picked commit had no order, so such fields always conflicted.
- Fuzz targets in `fuzz/` (`decode_object`, `merge_histories`), run for 60 s each
  in CI. A line-coverage gate (88%) runs in CI too.
- The crate's minimum supported Rust version is 1.88 (`rust-version`), checked
  in CI.
- `Repository::status` and `diff_working` no longer fail while a merge has
  conflicts. Conflicted entries are reported as conflicts, not as changes.

### Design decisions

- `tpt-primitives` is not `no_std` and has no `EntityId` or `ActorId`, so
  identities are local 16-byte values.
- `tpt-uir-core` has no generic value tree, so `UirNode` is defined in this
  crate. Its encoding is pinned by a test.
- A pending merge is saved only for a branch. A merge made while detached is
  lost when the repository is dropped.
