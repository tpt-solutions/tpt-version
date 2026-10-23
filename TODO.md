# tpt-version TODO

Source of truth: [spec.txt](spec.txt). Tick items as they are completed.
`[x]` done, `[~]` partly done (see the note), `[ ]` not started.

## Decisions (from clarification)
- Single crate `tpt-version`, Rust edition 2024, `#![no_std]` + `alloc` by default; features `std` (default), `s3`.
- Hash: SHA-256 via `tpt-crypto-hash::sha2::sha256`.
- Ed25519 via `tpt-crypto-sig::ed25519`. Small-order keys and commitments are rejected upstream (see open items).
- Git dependencies on `tpt-solutions/tpt-crypto` only (`tpt-crypto-hash`, `tpt-crypto-sig`). `tpt-testkit` is not used yet.
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
- [x] README, LICENSE files, CI. README, both license files and `.github/workflows/ci.yml` (test matrix on Linux/macOS/Windows, clippy, fmt, WASM no_std check) are written, and the CI run on GitHub is green.
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
- [x] API docs, CHANGELOG, crate metadata. CHANGELOG and crate metadata are done, `cargo doc` builds with `-D warnings`, and every public item is documented (`#![warn(missing_docs)]`, enforced by clippy in CI).
- [ ] v1.0.0 release prep. Not started. It needs the user's decision on the version and on publishing, since the git dependencies block crates.io. tpt-vertex integration happens in that repo.

## Testing & Quality (spec section 6)
- [x] Golden tests: `tests/golden.rs` pins the canonical bytes of an entity, tree and commit in `tests/fixtures/` via `tpt-testkit::golden::assert_bytes`. The CI matrix (Linux, macOS, Windows) checks them on every platform. `.gitattributes` forces LF in fixtures. The pinned hash in `tests/objects.rs` is kept as a SHA-256 known-answer check.
- [x] Property tests: random three-way merges checked against an independent oracle (`tests/random_merges.rs`). Random commit histories through `Repository` (branch, commit, merge, resolve, then commit) are checked in `tests/random_histories.rs`. Both use a custom xorshift generator (`tests/common`), not `tpt-testkit::property`.
- [x] Diff accuracy tests against known tree pairs
- [~] Benchmarks: `benches/throughput.rs` (`cargo bench --bench throughput`, plain timers, no harness). Single run on this machine: commit 10,000 entities 22.8 ms; diff two 10,000-entity trees 28.8 ms; merge two divergent branches 33.6 ms. Not statistical, and not run in CI. Criterion or tpt-testkit::bench would give proper numbers.
- [x] `no_std` build check on a WASM target (also checked on `thumbv7em-none-eabihf`)

## Open items to confirm later
- [x] Exact tpt-uir value/node types after reading the repo. Resolved as a deviation: `UirNode` is defined locally (see Decisions).
- [x] tpt-testkit is a dev-dependency, pinned to `tpt-solutions/tpt-testkit` rev `e77a901` (features `golden`, `property`). Used for golden fixtures and proptest round-trips (`tests/golden.rs`, `tests/round_trip.rs`). It is not in the normal dependency graph. It requires Rust 1.97, the pinned toolchain here. `bench` is not enabled, see the benchmarks item.
- [x] **Upstream bug in `tpt-crypto-sig` (fixed).** `VerifyingKey::verify` accepted small-order public keys, so an all-zero key and signature verified for any message. The fix is on `tpt-solutions/tpt-crypto` master as `8b73d9b`. This crate pins that revision in `Cargo.lock`, and the local workaround in `sign::verify` has been removed. `tests/history.rs` covers the forgery case.

## Review follow-ups (Oct 2026 platform review)
Plan: see the review notes. Tier 1 and Tier 2 come first.

### Tier 1: bugs and hazards
- [x] `FileSystemStore::atomic_write`: unique temp file names (pid + counter) and `sync_all` before rename. Done; covered by the concurrency test in `tests/refs.rs`.
- [x] Ref updates are last-writer-wins. Added `RefStore::compare_and_swap_ref` (FS uses a `.lock` file, S3 uses ETag `If-Match`), used by `commit`, `move_to` and `branch`; added `RepoError::RefMoved`.
- [x] Branch names: reject `a` vs `a/b` prefix clashes, case-only collisions (Windows/macOS) and Windows reserved names (`CON`, `NUL`...). Test on the Windows CI leg.
- [x] Nesting depth: decode caps at 128 but encode does not, so `put_entity` / `resolve_conflict(Custom)` can store an entity that can never be loaded. Enforce the cap on write.
- [x] Floats: compare by bit pattern in merge and diff, so an unchanged `NaN` field is not a phantom conflict/change. Update the `Scalar` docs.
- [x] `find_merge_base` / `is_ancestor` are roughly O(n^2) commit loads (each re-verifies signatures). Use a single two-sided BFS, a commit cache, optionally generation numbers; add a deep-history bench.
- [x] `Repository::commit`: no-op and detached commits are allowed and documented on `commit()`. The author vs signer relation is documented on `Commit::author`.
- [x] `put_entity`: nothing checks `entity.id()` against an id embedded in the UIR. Add a check or an `Entity` hook, and document it.
- [x] `S3Store`: `contains` turns errors into `false`; S3 has no `RefStore`, so branches cannot live in S3. Surface errors and add a ref store (conditional PUT / ETag CAS).
- [x] Tests for: FS concurrency, deep history performance, S3 + `Repository` end to end, ref-name collisions. Deep history is a bench case (`deep_history` in `benches/throughput.rs`, 18.7 ms release).

### Tier 2: adoption
- [x] Examples: `spreadsheet_cells`, `document_blocks`, `file_store_cli`, `signed_history`, `s3_sync`; each is linked from the README (`cargo run --example <name>`; `s3_sync` needs `--features s3`).
- [ ] `#[derive(Entity)]` (derive crate or `macro_rules!`) to remove hand-written `to_uir` / `from_uir`.
- [x] `prelude` module; the README is the crate docs, so its Quick Start is a doctest.
- [~] App template (entity + repo + merge-UI callback): not started. The "porting your data model" guide is done (`docs/porting.md`).
- [x] `docs/` guide: `docs/concepts.md` (includes store selection and no_std/WASM), `docs/conflicts.md`, `docs/porting.md` and `docs/faq.md` (versus Git and CRDTs).
- [ ] WASM demo page showing branch / edit / merge in the browser.
- [ ] Release prep: publishable dependencies (tpt-crypto, tpt-testkit) so `cargo add tpt-version` works.

### Tier 3: missing features
- [ ] `status`, `diff_working`, `reset`, `amend`, `revert`, `cherry_pick`.
- [ ] Tags (immutable refs), optionally signed.
- [ ] Per-entity `history` and field-level `blame`.
- [x] `fsck` (verify objects, signatures, refs) and `gc` (reachability). `maintenance::fsck` collects every problem; `Repository::gc` roots on refs, HEAD, the working copy and any pending merge, and refuses to delete if a reachable object is missing. S3 does not implement `PruneStore` yet, so gc is FS and memory only.
- [ ] `fetch` / `push` sync between stores, with a `Remote` abstraction.
- [ ] Signature trust policy (`TrustedKeys`, require signed commits per branch).
- [ ] Recursive merge for criss-cross histories.
- [ ] Hierarchical trees for large models; packfiles / delta compression for the FS store.

### Tier 4: innovative
- [ ] Sequence-aware list merge with per-field list policy (`Atomic`, `Set`, `Ordered`).
- [ ] Per-field merge strategies (counter sum, max, last-writer-wins, text three-way) declared on `Entity`.
- [ ] Post-merge semantic validators (constraints, formula references).
- [ ] Time-travel queries (`entity_at`, `as_of`) and diff-of-diffs for merge review.
- [ ] Signed review / approval objects stored in the CAS.
- [ ] `serde` / JSON / HTML diff output for a web viewer.
- [ ] Live-collaboration op log on top of the DAG.

### Tier 5: automation and usability
- [ ] `tpt` CLI (`init`, `commit`, `log`, `diff`, `merge`, `branch`, `resolve`, `fsck`) behind a `cli` feature.
- [~] CI: `cargo doc -D warnings` is done. Still open: `cargo deny` / audit, MSRV check, coverage, bench regression threshold.
- [ ] `cargo-fuzz` targets for `Object::decode` and `merge_trees`.
- [ ] Release automation (`release-plz` / `cargo-release`) and Dependabot for the pinned git revs.
- [~] Error messages that suggest the next action; `#[non_exhaustive]` on error enums is done.
- [ ] `pre_commit` / `post_merge` hook traits.
