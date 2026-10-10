# tpt-version TODO

Source of truth: [spec.txt](spec.txt). Tick items as they are completed.

## Decisions (from clarification)
- Single crate `tpt-version`, Rust edition 2024, `#![no_std]` + `alloc` by default; features `std`, `s3`.
- Hash: SHA-256 (via tpt-crypto-hash).
- Deps: git dependencies on `tpt-solutions/{tpt-uir (tpt-uir-core), tpt-primitives, tpt-crypto, tpt-testkit (dev)}`; `tpt-model` has no code, so a local `Entity` trait stands in (`id()`, `to_uir()`, `from_uir()`).
- Refs via a separate `RefStore` trait (`CasStore` stays as in the spec).
- Repository holds a staged working copy (`set_entity` / `remove_entity`); `commit()` snapshots it, `checkout()` loads a tree into it.
- Conflicts are stored CAS objects (base/ours/theirs hashes + field paths) referenced from the merged tree.
- Clock and Signer are injected traits; Ed25519 comes from tpt-crypto-sig.
- Trees are flat. Delete-vs-modify is a conflict. Nothing is resolved silently.
- License MIT OR Apache-2.0. No git init unless requested.
- `S3Store` is std-only behind the `s3` feature, using a pluggable client trait.

## Phase 0: Setup
- [ ] Read tpt-uir, tpt-primitives, tpt-crypto sources to confirm type names (UirNode/value, EntityId, ActorId, Signature, hash API)
- [ ] `Cargo.toml` (features, git deps, lints, license), `src/lib.rs` with `no_std` + `alloc`
- [ ] Module skeleton: object, store, dag, diff, merge, repo, error
- [ ] README, LICENSE files, CI (build native + `wasm32-unknown-unknown` no_std check, clippy, tests)
- [ ] Local `Entity` trait (tpt-model stand-in)

## Phase 1: Objects & CAS
- [ ] `ContentHash` newtype (SHA-256), hex Display/FromStr
- [ ] Canonical, deterministic serialization of Blob / Entity / Tree / Commit (with type tag in hashed bytes)
- [ ] `Blob`, `Entity` object (UIR node), `Tree { entries: BTreeMap<EntityId, ContentHash> }`, `Commit`
- [ ] Encode/decode round-trip for every object type
- [ ] `StoreError`; `CasStore` trait (`put`/`get`/`contains`)
- [ ] Verify hash on `get`/`put` (reject mismatches)
- [ ] `MemoryStore`
- [ ] `FileSystemStore` (`.tpt/objects/<prefix>/<rest>`, atomic writes, std feature)
- [ ] `RefStore` trait + Memory and FS impls (`.tpt/refs/heads/<name>`)
- [ ] Tests: store/retrieve, dedup, corrupt-object detection
- [ ] Milestone: store and retrieve objects by hash

## Phase 2: DAG & Branching
- [ ] Commit load helpers; ancestor iterator (BFS/DFS, de-duplicated)
- [ ] Descendant walk
- [ ] Branch create / list / delete / resolve (name validation)
- [ ] `Repository` skeleton: staged working copy, `commit()`, `log()`
- [ ] Injected `Clock` trait for timestamps
- [ ] Tests: linear history, branching, log order
- [ ] Milestone: linear commit history and branches

## Phase 3: Diffing
- [ ] `DiffEntry` (Added / Removed / Modified), `EntityDiff`, `FieldChange`
- [ ] Tree diff (sorted merge walk over two BTreeMaps)
- [ ] Entity diff: recursive field-path walk over UIR value trees
- [ ] Handle type changes, list changes, added/removed fields
- [ ] Tests: vertex moved (0,0,0) → (1,1,1) is Modified, not Removed + Added
- [ ] Milestone: show what changed between two commits

## Phase 4: Merging
- [ ] `find_merge_base` (handle criss-cross / multiple bases deterministically)
- [ ] `merge_trees` entity-level cases (only ours, only theirs, identical, add/add, delete/modify)
- [ ] Field-level 3-way merge; non-overlapping fields auto-merge
- [ ] Conflict objects (field path, base/ours/theirs values) stored in the CAS and referenced from the merged tree
- [ ] `MergeResult { merged_tree, conflicts }`, `MergeError`
- [ ] `Repository::merge`, fast-forward, merge commit with two parents
- [ ] `Repository::checkout`, conflict resolution hook for the app layer
- [ ] Tests: no data loss, conflict detection, merge commutativity where expected
- [ ] Milestone: merge two divergent branches

## Phase 5: Signing & Integration
- [ ] `Signer` trait; Ed25519 impl via tpt-crypto-sig
- [ ] Sign commits (signature excluded from the signed payload); verify on load
- [ ] Decide whether the signature is part of the commit hash (document it)
- [ ] `S3Store` behind `s3` feature (client trait, std only)
- [ ] Integration example: CAD-like entity set with branch / edit / merge
- [ ] API docs, CHANGELOG, crate metadata
- [ ] v1.0.0 release prep (tpt-vertex integration happens in that repo)

## Testing & Quality (spec section 6)
- [ ] Golden tests: fixed entity → fixed hash (tpt-testkit::golden); run on Linux, macOS, Windows in CI
- [ ] Property tests: random histories, branch, change, merge (tpt-testkit::property)
- [ ] Diff accuracy tests against known tree pairs
- [ ] Benchmarks: commit 10,000 entities, diff two large trees, merge two divergent branches (tpt-testkit::bench)
- [ ] `no_std` build check on a WASM target

## Open items to confirm later
- [ ] Exact tpt-uir value/node types after reading the repo (may adjust diff design)
- [ ] Whether tpt-testkit is usable as a git dev-dependency (`publish = false`, placeholder repo URL)
