# Conflict cookbook

A conflict means two sides changed the same thing differently, and the engine
will not choose for you. This page lists the common cases, what the merge does,
and how to settle each one.

After `merge`, `result.conflicts` lists what needs a decision, and
`repo.conflicts()` lists what is still pending. `commit` is refused until every
conflict is resolved with `resolve_conflict`.

## Same field, different values

Both sides set `text` on one block:

- Main sets `"Width (cm)"`, the branch sets `"Width (in)"`.
- The merge keeps the base value and records a conflict on `text`.

Resolve it with:

- `Resolution::Ours` to keep main's value.
- `Resolution::Theirs` to keep the branch's value.
- `Resolution::Custom(node)` for a value neither side wrote. Build it from the
  two versions, for example by asking a person in a merge UI.

Use `Custom` when the right answer is a third value. Do not pick a side just to
make the commit go through.

## Delete versus edit

One side removes an entity while the other side changes it. The merge keeps the
edit and records a conflict, because the edit may be the work that should
survive.

- `Ours` keeps our side. If we deleted the entity, the entity is removed.
- `Theirs` keeps their side. If they deleted it, the entity is removed.

## A list field changed on both sides

Lists are compared as one value. If both sides change `tags`, the merge cannot
tell whether to combine the tags, in what order, or whether a removal should
win. It records a conflict on `tags`, even when the two sides added different
tags.

If you want additions to combine, the usual answer is a `Custom` value that
takes the union of both sides. Read both versions with `conflict.fields` and
build the union in application code. If you want that to happen everywhere,
model the tags as a set of entities with their own ids, one per tag. Then an
add on each side is a separate entity and merges cleanly.

## Two new entities with the same id

Both sides create an entity with the same id but different content. There is no
base version, so the merge compares the two sides field by field. Each field
that differs is a conflict. A field that both sides set to the same value merges
cleanly. Ids should be unique. Generate them once, when the entity is created,
and never derive them from content that can change.

## Edits that touch different fields

No conflict. Main changes `position.x` and the branch changes `position.y`.
The merged entity has both changes. This is the main reason to keep entities
small: the more an entity holds, the more often two edits touch the same entity,
though they rarely touch the same field.

## Reading a conflict

A `Conflict` carries the entity id, the hashes of the base, ours and theirs
versions, and one `FieldConflict` per disputed field. Each `FieldConflict` has
the field path, and the base, ours and theirs values at that path. Use these to
show a person what they are choosing between.

```rust
for conflict in &result.conflicts {
    for field in &conflict.fields {
        println!("{}: ours {:?}, theirs {:?}", field.path.join("."), field.ours, field.theirs);
    }
}
```

The example [`document_blocks`](../examples/document_blocks.rs) walks through a
list conflict end to end, and [`spreadsheet_cells`](../examples/spreadsheet_cells.rs)
shows a `Custom` resolution.
