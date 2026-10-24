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
- [x] v1.0.0 release prep. Version is 1.0.0 in `Cargo.toml`, and the changelog section is written. Nothing is tagged or published: `publish = false` stays until the git dependencies are published. tpt-vertex integration happens in that repo.

## Testing & Quality (spec section 6)
- [x] Golden tests: `tests/golden.rs` pins the canonical bytes of an entity, tree and commit in `tests/fixtures/` via `tpt-testkit::golden::assert_bytes`. The CI matrix (Linux, macOS, Windows) checks them on every platform. `.gitattributes` forces LF in fixtures. The pinned hash in `tests/objects.rs` is kept as a SHA-256 known-answer check.
- [x] Property tests: random three-way merges checked against an independent oracle (`tests/random_merges.rs`). Random commit histories through `Repository` (branch, commit, merge, resolve, then commit) are checked in `tests/random_histories.rs`. Both use a custom xorshift generator (`tests/common`), not `tpt-testkit::property`.
- [x] Diff accuracy tests against known tree pairs
- [~] Benchmarks: Criterion in `benches/throughput.rs` (`cargo bench --bench throughput`). CI compiles them but does not run them. A short local run (1 s measurement) gave: commit 10,000 entities 30.7 ms; diff two 10,000-entity trees 32.9 ms; merge two divergent branches 54.4 ms; merge two long histories 47.1 ms. Run longer before quoting numbers. A regression threshold in CI is still open.
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
- [x] `#[derive(Entity)]` (derive crate or `macro_rules!`) to remove hand-written `to_uir` / `from_uir`. It is a proc-macro in `derive/`, behind the default `derive` feature. Each field type implements `UirValue`, which covers scalars, `EntityId`, `Option`, `Vec` and fixed arrays.
- [x] `prelude` module; the README is the crate docs, so its Quick Start is a doctest.
- [x] App template (entity + repo + merge-UI callback). `examples/app_template.rs` has a derived entity, a `Workspace` that owns the repository, and a `MergeUi` trait the app implements. The "porting your data model" guide is `docs/porting.md`.
- [x] `docs/` guide: `docs/concepts.md` (includes store selection and no_std/WASM), `docs/conflicts.md`, `docs/porting.md` and `docs/faq.md` (versus Git and CRDTs).
- [x] WASM demo page showing branch / edit / merge in the browser. `wasm/` has the wasm-bindgen crate, and `wasm/www/index.html` drives it. Run in headless Microsoft Edge on Windows: a smoke page covering commit, branch, switch and merge passed all eight checks, and the demo page loads with no errors. The smoke page is not in the repo. Build steps are in `wasm/README.md`.
- [ ] Release prep: publishable dependencies (tpt-crypto, tpt-testkit) so `cargo add tpt-version` works.

### Tier 3: missing features
- [x] `status`, `diff_working`, `reset`, `amend`, `revert`, `cherry_pick`. Only `reset_hard` exists. Soft and mixed resets are not implemented. `revert` and `cherry_pick` stage a three-way merge for review, so a clash becomes a conflict.
- [x] Tags (immutable refs), optionally signed. `create_tag` makes a lightweight tag. `create_annotated_tag` makes an `AnnotatedTag` object with a message and an optional signature. `resolve_tag` returns the commit for either kind, and `annotated_tag` returns the signed details. Tags live under the reserved `tags/` prefix and are immutable. Covered by `tests/signed_tags.rs`.
- [x] Per-entity `history` and field-level `blame`: `Repository::entity_history` and `Repository::field_blame`, along the first-parent chain from the head.
- [x] `fsck` (verify objects, signatures, refs) and `gc` (reachability). `maintenance::fsck` collects every problem; `Repository::gc` roots on refs, HEAD, the working copy and any pending merge, and refuses to delete if a reachable object is missing. S3 does not implement `PruneStore` yet, so gc is FS and memory only.
- [x] `fetch` / `push` sync between stores (`sync::push`, `sync::fetch`). There is no separate `Remote` trait: any `CasStore` + `RefStore` is a remote. Only the reachable objects the destination lacks are copied, and the ref moves by compare-and-swap, so non-fast-forward updates are refused.
- [x] Signature trust policy (`TrustPolicy` in `trust`, require signed commits per branch). Commits to a governed branch and merges into it need a trusted signature. Commits made before the policy are not re-checked, and a branch outside the policy is unaffected.
- [x] Recursive merge for criss-cross histories. `dag::merge_bases` returns every best base. With several, they are merged into a virtual base (conflicts settled as ours) before the three-way merge. Covered by `tests/criss_cross.rs`.
- [~] Hierarchical trees for large models: done. A snapshot above 256 entities is stored as `Object::TreeRoot` (tag 7), with one shard per first byte of the entity id's hash. Each shard is an ordinary tree, so an unchanged shard keeps its hash, and a small edit stores about a third of the flat size at 600 entities. `load_tree`, merges, fsck, gc and diff all read both layouts, and small snapshots keep their old bytes. Packfiles and delta compression are not done. Covered by `tests/sharding.rs`.

### Tier 4: innovative
- [~] Sequence-aware list merge with per-field list policy (`Atomic`, `Set`, `Ordered`). `ListStrategy::Ordered` is a three-way merge over items, so edits at different positions combine, and overlapping edits conflict (`text_merge::merge_sequence`, `tests/merge_policy.rs`). `Atomic` (the default) and `Set` (`Union`) are also there. Won't do: move detection. Items in a list have no identity of their own, so a move cannot be told apart from a removal and an addition when values repeat. A moved item shows as a removal plus an addition.
- [x] Per-field merge strategies. `MergePolicy` on the repository (`with_merge_policy`) gives numeric fields `Sum`, `Max` or `Min`, text fields `TextLines`, lists `ListStrategy::Union`, and any field `LastWriterWins` (the newer commit by timestamp, then hash, wins). Cherry-pick now compares head with the picked commit, so it follows the same rule. Decided, not built: (1) strategies declared on `Entity`. Merges work on UIR and do not know entity types, so this would need a type tag in the stored object format; the repository-level policy stays the way to set strategies. (2) `LastWriterWins` on the virtual-base path. A virtual base is an ancestor, not a side, so it has no newer side. Its conflicts still settle as ours. (3) `LastWriterWins` on revert. The reverted commit has no side of its own, so such fields conflict. Covered by `tests/merge_policy.rs`.
- [x] Post-merge semantic validators. `Validator` (`src/validate.rs`) sees the whole snapshot, so rules can span entities. Attached with `Repository::with_validator`, it refuses commits while it reports problems (`RepoError::ValidationFailed`), and `Repository::validate` runs it on demand. Only commits are checked, not sync or fetch. Covered by `tests/validators.rs`.
- [x] Time-travel queries: `entity_at` (by commit), `commit_as_of` and `entity_as_of` (by timestamp). Merge review: `Repository::merge_review` shows a pending merge's result against each side (`MergeReview::from_head`, `from_incoming`). Covered by `tests/diff_merge.rs`.
- [x] Signed review / approval objects stored in the CAS. `Object::Review` (tag 6) holds a verdict on a commit, signed by the repository's signer. Reviews live under `reviews/<commit>/<reviewer>` refs, so gc and fsck see them. A reviewer's later review replaces their earlier one. `Repository::review`, `reviews_of`, and `Review::is_valid` (signature only, not trust). Covered by `tests/reviews.rs`.
- [x] Diff output for viewers: `export::diff_to_json`, `node_json` for values, and `export::diff_to_html` for a table with escaped content. Written by hand with no serde dependency, by design.
- [x] Live-collaboration op log. `oplog` has an `OpLog` of Lamport-stamped puts and removes, and `Replica` for local edits. Each operation carries a per-replica sequence number and the counts its author had seen, so `Replica::deliver` applies it only after its dependencies, and holds it otherwise. Logs save to a store with `OpLog::save` and `OpLog::load`, as `Object::OpLog`. Covered by `tests/oplog.rs`. Transport is left to the caller, by design: the crate has no network layer.

### Tier 5: automation and usability
- [x] `tpt` CLI. `src/bin/tpt.rs`, behind the default `cli` feature, covers init, commit, get, list, log, diff, branch, branches, switch, merge, tag, tags, fsck and gc, over a key-value document. A merge that leaves conflicts is saved under `merges/<branch>/` refs, so `conflicts`, `resolve <key> ours|theirs` and `finish-merge` settle it across runs. `merge <branch> ours|theirs <message>` still settles every conflict at once. `revert` and `cherry-pick` stage their result the same way, so their conflicts also survive between runs. Covered by `tests/cli.rs`.
- [x] CI: `cargo doc -D warnings`, `cargo deny` (`deny.toml`), an MSRV check (Rust 1.88, the let-chain minimum, library and binaries only), a line-coverage gate (88%, measured at 90.4%), and a fuzz smoke job. Still open: a benchmark regression threshold.
- [~] `cargo-fuzz` targets in `fuzz/`: `decode_object` (canonical round trip) and `merge_histories` (random edit, branch, merge and commit sequences, then conflicts resolved and fsck run). CI runs each for 60 s on Linux. They do not link on Windows MSVC, so they were checked here only by replaying 3,000 random inputs through the same code. Proptest in `tests/decode_robustness.rs` still covers `Object::decode` on every platform.
- [~] Dependabot already covers GitHub Actions and both cargo crates (`.github/dependabot.yml`). The git revs are bumped by hand. Release automation (`release-plz` / `cargo-release`) waits until the crate can be published.
- [x] Error messages that suggest the next action, and `#[non_exhaustive]` on error enums. The repository errors name the call that fixes them, such as `resolve_conflict`, `reset_hard` or `list_branches`.
- [x] `pre_commit` / `post_merge` hook traits. `Hooks` on `Repository::with_hooks`; a refused `pre_commit` returns `RepoError::HookRejected`.
