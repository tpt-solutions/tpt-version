//! Branches, edits and merges a small CAD-style entity set.
//!
//! Run with `cargo run --example cad_branch_merge`.

use std::collections::BTreeMap;

use tpt_version::diff::diff_commits;
use tpt_version::error::ObjectError;
use tpt_version::{
    ActorId, DiffEntry, Ed25519Signer, Entity, EntityId, LogicalClock, MemoryStore, RepoError,
    Repository, Resolution, Scalar, UirNode,
};

/// A CAD vertex. Its id is stored in the node so it survives a round trip.
struct Vertex {
    id: EntityId,
    position: [f64; 3],
}

impl Entity for Vertex {
    fn id(&self) -> EntityId {
        self.id
    }

    fn to_uir(&self) -> UirNode {
        let position = ["x", "y", "z"]
            .into_iter()
            .zip(self.position)
            .map(|(axis, value)| (axis.into(), UirNode::from(value)))
            .collect();
        UirNode::Map(BTreeMap::from([
            (
                "id".into(),
                UirNode::Scalar(Scalar::Bytes(self.id.as_bytes().to_vec())),
            ),
            ("position".into(), UirNode::Map(position)),
        ]))
    }

    fn from_uir(node: &UirNode) -> Result<Self, ObjectError> {
        let UirNode::Map(fields) = node else {
            return Err(ObjectError::InvalidValue("vertex"));
        };
        let Some(UirNode::Scalar(Scalar::Bytes(id))) = fields.get("id") else {
            return Err(ObjectError::InvalidValue("vertex id"));
        };
        let Some(UirNode::Map(position)) = fields.get("position") else {
            return Err(ObjectError::InvalidValue("vertex position"));
        };
        let mut coords = [0.0; 3];
        for (slot, axis) in coords.iter_mut().zip(["x", "y", "z"]) {
            let Some(UirNode::Scalar(Scalar::Float(value))) = position.get(axis) else {
                return Err(ObjectError::InvalidValue("coordinate"));
            };
            *slot = *value;
        }
        let id = <[u8; 16]>::try_from(id.as_slice())
            .map_err(|_| ObjectError::InvalidValue("vertex id length"))?;
        Ok(Self {
            id: EntityId::from_bytes(id),
            position: coords,
        })
    }
}

fn vertex(id: u8, position: [f64; 3]) -> Vertex {
    Vertex {
        id: EntityId::from_bytes([id; 16]),
        position,
    }
}

fn main() -> Result<(), RepoError> {
    let author = ActorId::from_bytes([1; 16]);
    let signer = Ed25519Signer::from_seed([7; 32]);
    let a = EntityId::from_bytes([1; 16]);
    let mut repo = Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main")?
        .with_signer(Box::new(signer));

    repo.put_entity(&vertex(1, [0.0, 0.0, 0.0]))?;
    repo.put_entity(&vertex(2, [5.0, 0.0, 0.0]))?;
    let base = repo.commit("initial layout", &author)?;
    repo.branch("feature", base)?;

    // Main moves A along x.
    repo.put_entity(&vertex(1, [1.0, 0.0, 0.0]))?;
    repo.commit("move A along x", &author)?;

    // Feature moves B along x and raises A along z. The edits touch different
    // fields of A, so the merge combines them without a conflict.
    repo.switch_branch("feature")?;
    repo.put_entity(&vertex(2, [6.0, 0.0, 0.0]))?;
    repo.put_entity(&vertex(1, [0.0, 0.0, 9.0]))?;
    repo.commit("nudge B, raise A", &author)?;

    repo.switch_branch("main")?;
    let merged = repo.merge("feature")?;
    println!(
        "fast automatic merge: {} conflict(s)",
        merged.conflicts.len()
    );
    let merge_head = repo.commit("merge feature", &author)?;
    let a_now: Vertex = repo.entity(&a)?.expect("A is staged");
    println!("A after merge: {:?}", a_now.position);

    // Both sides now change the same field of A, which is a real conflict.
    repo.branch("rival", merge_head)?;
    repo.put_entity(&vertex(1, [4.0, 0.0, 0.0]))?;
    repo.commit("A to x=4 on main", &author)?;
    repo.switch_branch("rival")?;
    repo.put_entity(&vertex(1, [-4.0, 0.0, 0.0]))?;
    repo.commit("A to x=-4 on rival", &author)?;
    repo.switch_branch("main")?;

    let result = repo.merge("rival")?;
    for conflict in &result.conflicts {
        let fields: Vec<String> = conflict.fields.iter().map(|f| f.path.join(".")).collect();
        println!("conflict on {} at {}", conflict.entity, fields.join(", "));
    }
    // Committing now would fail until the conflict is resolved.
    repo.resolve_conflict(&a, Resolution::Ours)?;
    let final_head = repo.commit("merge rival, keep main's A", &author)?;

    println!("history on main:");
    for commit in repo.log("main")? {
        println!("  {}", commit.message);
    }

    println!("changes since the initial layout:");
    for entry in diff_commits(repo.store(), &base, &final_head)? {
        match entry {
            DiffEntry::Added { id, .. } => println!("  added {id}"),
            DiffEntry::Removed { id, .. } => println!("  removed {id}"),
            DiffEntry::Modified { id, diff, .. } => {
                for change in diff.field_changes {
                    println!(
                        "  {id} {}: {:?} -> {:?}",
                        change.field_path.join("."),
                        change.old_value,
                        change.new_value
                    );
                }
            }
        }
    }
    Ok(())
}
