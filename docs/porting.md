# Porting your data model

This guide is for putting an existing model under version control. Most of the
work is in three decisions: what counts as one entity, where ids come from, and
which fields are lists. Get those right and merges mostly take care of
themselves.

## 1. Choose the entity granularity

An entity is the unit that is versioned, diffed and merged. Two people changing
the same entity is a candidate conflict. Two people changing different entities
never conflict.

- **Too coarse**: one entity holds the whole document or the whole assembly.
  Any two edits meet at the same entity. Field-level merge still helps, but the
  conflict list grows with the size of the thing.
- **Too fine**: every number is an entity. Diffs and trees get large, and
  references between entities break when one of them is merged away.

A good starting point is the thing a person would name in a sentence: a vertex,
a cell, a paragraph, a part. Split an entity when two people routinely edit
different halves of it. Merge two entities when they always change together.

## 2. Give every entity a stable id

The id identifies the entity across every version. It must be:

- **Assigned once**, when the entity is created, and never reused.
- **Independent of content**. Do not derive it from a name or a position that
  an edit can change. A hash of the content changes with every edit and would
  look like a delete plus an add.
- **Stored in the entity**. `put_entity` reads the stored form back and refuses
  it if the id differs from `id()`. Put the id in `to_uir` and read it in
  `from_uir`, as the examples do.

Use a random or time-ordered 16-byte id. Local counters are fine on one machine
but collide across machines.

## 3. Map fields to `UirNode`

`UirNode` has three shapes:

| Shape | Use for | Merge behaviour |
| --- | --- | --- |
| `Scalar` | numbers, booleans, text, bytes, null | compared as one value |
| `Map` | records with named fields | merged field by field |
| `List` | ordered sequences | compared as one value |

Make a field a **map entry** when it can change independently of its siblings.
Two people setting different map fields merge cleanly. Make a field a **list**
only when its order carries meaning and it changes as a unit. Any two concurrent
changes to a list conflict, even if they add different items. If items are
added and removed by different people, model each item as its own entity with
its own id.

## 4. Floats

Floats compare and encode by bit pattern. Two identical `NaN`s are not a
change, `0.0` and `-0.0` are different content, and a value that is rounded
on one machine and not on another is a real change. Round values before
storing them if you need them to compare equal across machines.

## 5. Keep decoding tolerant

Stored objects stay readable as long as `from_uir` accepts them. There is no
schema versioning yet. When you add a field:

- Make `from_uir` treat a missing field as its default, rather than an error.
- Do not reuse a field name for a different meaning.
- Keep old entities decodable until every branch that might hold them is gone.

## 6. Mind the nesting limit

Decoding accepts at most 128 levels of nesting. A deeper value is refused when
it is staged, with `ObjectError::InvalidValue("nesting depth")`. A model that
needs deeper nesting should flatten part of it into separate entities, each
with its own id.

## Checklist

- [ ] One entity per independently edited thing, with a stable 16-byte id.
- [ ] The id is in `to_uir` and read back in `from_uir`.
- [ ] Fields that change separately are map entries, not list items.
- [ ] Lists that people edit concurrently are modelled as entities.
- [ ] Floats are rounded where cross-machine equality matters.
- [ ] `from_uir` tolerates missing fields.
- [ ] Nesting stays within 128 levels.

For the conflict cases each choice leads to, see [conflicts.md](conflicts.md).
