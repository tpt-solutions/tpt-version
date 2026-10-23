# Concepts

tpt-version stores versions of entities. An entity is a value with a stable id,
such as a CAD vertex or a spreadsheet cell. The engine keeps every version as
content-addressed objects, links them into a commit graph, and merges branches
field by field.

## Objects

Everything is stored under the SHA-256 of its canonical bytes:

- **Entity**: one `UirNode` value, the encoded form of one entity.
- **Tree**: a snapshot. It maps each entity id to the hash of its entity.
- **Commit**: a tree hash, parent hashes, an author, a timestamp, a message and
  an optional signature.
- **Conflict**: a stored record of a merge that could not decide a field.

Identical content has an identical hash and is stored once. Changing any byte
of an object changes its hash, so corruption is detected when the object is read.

## Entities and ids

An entity implements `Entity`: `id()`, `to_uir()` and `from_uir()`. The id must
be stable for the life of the entity. The stored form must also carry that id,
because `put_entity` reads the stored form back and refuses it when the id
differs. See [porting.md](porting.md) for choosing ids and granularity.

## Branches, commits and the working copy

A `Repository` has a staged working copy. `put_entity` and `remove_entity`
change the working copy. `commit` snapshots it into a new commit and moves the
current branch. Branches are named refs to commits.

Ref updates are compare-and-swap. A repository that read a branch and then
finds it moved refuses the commit with `RepoError::RefMoved`. It does not
overwrite the other writer. Reopen the branch and try again.

`checkout` moves to a commit with no branch, and a commit made there is
reachable only through its hash. Call `branch` on that hash to keep it.

## Merging

`merge(branch)` finds the best common ancestor and then merges three ways, per
entity:

- An entity changed on one side only takes that side's value.
- An entity changed identically on both sides takes that value.
- An entity changed on both sides is merged field by field. Fields changed on
  one side only take that side's value.
- A field changed on both sides to different values is a **conflict**.
- Deleting an entity on one side while the other side changed it is a conflict.

A list field is one value. Two sides that each change a list conflict, even if
the changes add different items. See [conflicts.md](conflicts.md).

Nothing is resolved silently. `commit` is refused while conflicts remain, and
each one is settled with `resolve_conflict`, using `Ours`, `Theirs` or a value
the application builds.

## Diffs

`diff_commits` and `diff_entities` report added, removed and modified entities.
A modified entity lists its changed field paths, with the old and new values at
each path. A moved vertex is one modified entity, not a removal and an addition.

## Signing

With a `Signer` on the repository, every commit is signed with Ed25519 over its
canonical bytes, and the signature is part of the commit hash. Every signature
is verified when a commit is loaded. A commit that fails verification is refused
with `RepoError::InvalidSignature`.

The signature proves which key signed a commit. It does not say whether that
key belongs to a particular person. Deciding which keys to trust is the
application's job. A trust policy is on the roadmap.

## Clocks

Commits record a timestamp from a `Clock`. `LogicalClock` counts up from a value
and suits tests and reproducible output. `SystemClock` reads the system time and
suits real use with the file system store. Timestamps are used for one thing:
choosing among several merge bases in a criss-cross history. A clock that goes
backwards does not corrupt history, but it can change which base such a merge
picks.

## Choosing a store

| Store | Feature | Use it for |
| --- | --- | --- |
| `MemoryStore` | always | tests, short-lived repositories, WASM |
| `FileSystemStore` | `std` (default) | a repository on one machine |
| `S3Store` | `s3` | sharing a repository between machines |

`S3Store` takes an `ObjectClient` and, for branch refs, a `ConditionalClient`.
You implement these over your S3 SDK. The conditional methods must map to
`If-None-Match: *` and `If-Match` with the ETag that was read. Without that
mapping, branch updates are not safe across machines.

Every backend verifies bytes against their hash on write and on read.

## `no_std` and WASM

The crate is `no_std` with `alloc` by default. Turn off the default `std`
feature for embedded or WASM targets:

```sh
cargo check --no-default-features --target wasm32-unknown-unknown
```

Without `std`, you get the object model, the merge and diff engines,
`MemoryStore`, `LogicalClock` and `Ed25519Signer`. The file system store and
`SystemClock` need `std`. A `no_std` target has no clock to read, so it supplies
its own `Clock`, and its own seed for `Ed25519Signer`.
