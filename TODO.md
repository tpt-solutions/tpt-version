# tpt-version TODO

Source of truth: [spec.txt](spec.txt). Tick items as they are completed.
`[x]` done, `[~]` partly done (see the note), `[ ]` not started.

## Decisions (from clarification)
- Single crate `tpt-version`, Rust edition 2024, `#![no_std]` + `alloc` by default; features `std` (default), `s3`.
- Hash: SHA-256 via `tpt-crypto-hash::sha2::sha256`.
- Ed25519 via `tpt-crypto-sig::ed25519`, with a strict small-order check added in `sign::verify` (see open items).
- Git dependencies on `tpt-solutions/tpt-crypto` only (`tpt-crypto-hash`, `tpt-crypto-sig`, `tpt-crypto-curve`, `tpt-crypto-field`). `tpt-testkit` is not used yet.
- **Deviation:** `tpt-primitives` is not `no_std` and has no `EntityId`/`ActorId` types, so it is not a dependency. `EntityId` and `ActorId` are local 16-byte identifiers.
- **Deviation:** `tpt-uir-core` has no generic value tree (only IR ops and attributes), so `UirNode` (scalar / list / map) is defined in this crate. Its encoding is pinned by a test.
- `tpt-model` has no code, so a local `Entity` trait stands in (`id()`, `to_uir()`, `from_uir()`).
- Refs via a separate `RefStore` trait (`CasStore` stays as in the spec).
- Repository holds a staged working copy (`put_entity` / `remove_entity`); `commit()` snapshots it, `checkout()` loads a commit into it.
- Conflicts are stored CAS objects (base/ours/theirs hashes + field paths and values) referenced from the merged tree.
- Commit signatures are part of the commit hash. The signature covers the commit encoded without the signature field.
- Clock and Signer are injected traits.
- Trees are flat. Delete-vs-modify is a conflict. Nothing is resolved silently: commits are refused while conflicts remain.
- License MIT OR Apache-2.0. No git init unless requested.
- `S3Store` is std-only behind the `s3` feature, using a pluggable `ObjectClient` trait.

## Phase 0: Setup
- [x] Read tpt-uir, tpt-primitives, tpt-crypto sources to confirm type names. Results are in the Decisions section above.
- [x] `Cargo.toml` (features, git deps, lints, license), `src/lib.rs` with `no_std` + `alloc`
- [x] Module skeleton: object, store, dag, diff, merge, repo, error (plus codec, uir, sign, entity)
- [~] README, LICENSE files, CI. README, both license files and `.github/workflows/ci.yml` (test matrix on Linux/macOS/Windows, clippy, fmt, WASM no_std check) are written. The CI workflow has not run on GitHub yet.
- [x] Local `Entity` trait (tpt-model stand-in)

## Phase 1: Objects & CAS
- [x] `ContentHash` newtype (SHA-256), hex Display/FromStr
- [x] Canonical, deterministic serialization of Blob / Entity / Tree / Commit (with type tag in hashed bytes)
- [x] `Blob`, `Entity` object (UIR node), `Tree { entries: BTreeMap<EntityId, ContentHash> }`, `Commit`
- [x] Encode/decode round-trip for every object type
- [x] `StoreError`; `CasStore` trait (`put`/`get`/`contains`)
- [x] Verify hash on `get`/`put` (reject mismatches)
- [x] `MemoryStore`
- [x] `FileSystemStore` (`objects/<prefix>/<rest>` under the root, atomic writes via rename, std feature)
- [x] `RefStore` trait + Memory and FS impls (`refs/heads/<name>`)
- [x] Tests: store/retrieve, dedup, corrupt-object detection
- [x] Milestone: store and retrieve objects by hash

## Phase 2: DAG & Branching
- [x] Commit load helpers; ancestor iterator (BFS, de-duplicated)
- [x] Descendant walk
- [x] Branch create / list / delete / resolve (name validation)
- [x] `Repository` skeleton: staged working copy, `commit()`, `log()`
- [x] Injected `Clock` trait for timestamps
- [x] Tests: linear history, branching, log order
- [x] Milestone: linear commit history and branches

## Phase 3: Diffing
- [x] `DiffEntry` (Added / Removed / Modified), `EntityDiff`, `FieldChange`
- [x] Tree diff (ordered walk over the union of entity ids)
- [x] Entity diff: recursive field-path walk over UIR value trees
- [x] Handle type changes, added/removed fields. List changes are compared as one whole value (a documented choice).
- [x] Tests: vertex moved (0,0,0) → (1,1,1) is Modified, not Removed + Added
- [x] Milestone: show what changed between two commits

## Phase 4: Merging
- [x] `find_merge_base`. Criss-cross histories pick the newest base by timestamp, ties broken by hash, so the choice is deterministic (tested in `tests/history.rs`). There is no recursive merge of several bases.
- [x] `merge_trees` entity-level cases (only ours, only theirs, identical, add/add, delete/modify)
- [x] Field-level 3-way merge; non-overlapping fields auto-merge
- [x] Conflict objects (field path, base/ours/theirs values) stored in the CAS and referenced from the merged tree
- [x] `MergeResult { merged_tree, conflicts }`, `MergeError` (alias for `RepoError`)
- [x] `Repository::merge`, fast-forward, merge commit with two parents
- [x] `Repository::checkout`, `Repository::resolve_conflict` (Ours / Theirs / Custom) as the conflict hook for the app layer
- [x] Tests: no data loss, conflict detection, merge commutativity. The random oracle test in `tests/random_merges.rs` covers these over 400 seeds.
- [x] Milestone: merge two divergent branches

## Phase 5: Signing & Integration
- [x] `Signer` trait; Ed25519 impl via tpt-crypto-sig (`Ed25519Signer`)
- [x] Sign commits (signature excluded from the signed payload); verify on load
- [x] Decide whether the signature is part of the commit hash. Decision: it is, and the decision is documented on `Commit`.
- [x] `S3Store` behind `s3` feature (client trait, std only), tested with an in-memory fake
- [x] Integration example: CAD-like entity set with branch / edit / merge (`examples/cad_branch_merge.rs`)
- [~] API docs, CHANGELOG, crate metadata. CHANGELOG and crate metadata are done, and `cargo doc` builds with `-D warnings`. Per-item docs are not complete.
- [ ] v1.0.0 release prep. Not started. It needs the user's decision on the version and on publishing, since the git dependencies block crates.io. tpt-vertex integration happens in that repo.

## Testing & Quality (spec section 6)
- [~] Golden tests: fixed entity → fixed hash. One pinned hash is in `tests/objects.rs`, and it is checked against a SHA-256 known-answer vector. It has not been run on macOS or Windows yet (CI matrix pending). This uses a plain test, not `tpt-testkit::golden`.
- [~] Property tests: random three-way merges checked against an independent oracle (`tests/random_merges.rs`). This is a custom xorshift generator, not `tpt-testkit::property`. It covers tree-level merges only, not random commit histories through `Repository`.
- [x] Diff accuracy tests against known tree pairs
- [~] Benchmarks: `benches/throughput.rs` (`cargo bench --bench throughput`, plain timers, no harness). Single run on this machine: commit 10,000 entities 22.8 ms; diff two 10,000-entity trees 28.8 ms; merge two divergent branches 33.6 ms. Not statistical, and not run in CI. Criterion or tpt-testkit::bench would give proper numbers.
- [x] `no_std` build check on a WASM target (also checked on `thumbv7em-none-eabihf`)

## Open items to confirm later
- [x] Exact tpt-uir value/node types after reading the repo. Resolved as a deviation: `UirNode` is defined locally (see Decisions).
- [ ] Whether tpt-testkit is usable as a git dev-dependency (`publish = false`, placeholder repo URL). Not evaluated, because nothing needs it yet.
- [~] **Upstream bug in `tpt-crypto-sig`:** `VerifyingKey::verify` accepts small-order public keys. An all-zero key and signature verify for any message on the cofactored equation. `tpt-version` works around it in `sign::verify`, which rejects small-order keys and commitments. The fix is committed locally as `de7a923` on branch `fix/reject-small-order-ed25519-keys` in a `tpt-crypto` clone. It is exported as `0001-tpt-crypto-sig-reject-small-order-Ed25519-public-key.patch` in the session scratchpad. It has a regression test that fails without the fix. It is **not pushed** to `tpt-solutions/tpt-crypto`. Once it lands upstream and this crate's git dependency picks it up, remove the workaround in `sign::verify`.
