# FAQ

## How is this different from Git?

Git stores files and merges text lines. tpt-version stores entities and merges
their fields. Changing a CAD vertex's position in Git is a line change inside a
file, and two people editing different vertices in the same file can conflict
because their edits land near each other. Here, each vertex is its own entity,
so those edits never meet, and two people editing different fields of one
vertex merge automatically.

Git's model is also a good match for this crate. Both use a content-addressed
object store, a commit DAG and branches. The differences are the unit of merge
(entity and field, not line) and that a conflict is stored as data that the
application resolves, rather than as markers in a file.

## How is this different from CRDTs?

A CRDT is designed so that any two replicas converge without a conflict step.
tpt-version does not converge automatically. When two sides change the same
field, it records a conflict and waits for a decision. Nothing is resolved
silently.

That is a trade-off. CRDTs suit data where any merge is acceptable, such as
counters or sets. tpt-version suits data where a person has to choose, such as
the width of a part, and where you want the full history to be inspectable and
reversible. The planned per-field strategies (counter sum, maximum, text
three-way merge) would bring CRDT-like behaviour to chosen fields, opt-in per
type.

## Does it merge text?

Not yet. A text field is one value, so two edits to the same text field conflict
even when they touch different words. A text three-way merge is on the roadmap
as an opt-in field strategy. Until then, store long text as a list of entities,
one per paragraph, so that edits to different paragraphs do not conflict.

## Can I delete data?

Deleting an entity stages its removal, and a commit makes it part of history.
The old objects stay in the store, because history refers to them. There is no
garbage collection yet, so the store only grows. Garbage collection (`gc`) is
planned, and it will need a reachability walk from every ref.

## Why does a list conflict when the items differ?

Lists are compared as one value, because the merge cannot know the intended
order or whether a removal should win. See [conflicts.md](conflicts.md) for how
to settle it, and [porting.md](porting.md) for modelling the items as entities.

## Are signatures enough to prove who made a change?

They prove which key signed a commit. Whether that key belongs to a person is a
question of trust, which the engine does not answer yet. A signature trust
policy is planned. Until it exists, decide which keys to accept in your
application, and check the signer against that list.

## Can I run it in the browser?

The core compiles for `wasm32-unknown-unknown` with `--no-default-features`, and
`MemoryStore` works there. A browser demo is on the roadmap. See
[concepts.md](concepts.md) for which features need `std`.

## Is the object format stable?

The encoding is pinned by golden tests in `tests/fixtures`, and the hash of a
known object is checked in `tests/objects.rs`. A change that alters existing
hashes must be deliberate. The crate is pre-1.0, so the Rust API can still
change, but the stored format is treated as stable.
