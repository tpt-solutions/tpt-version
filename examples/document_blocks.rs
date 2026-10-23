//! A document as blocks, each with its own id. Edits to different fields of a
//! block merge. A list field such as `tags` is one value, so two concurrent
//! edits to it conflict even when they add different tags. That is deliberate:
//! the merge cannot know whether the tags should be combined, in what order, or
//! whether one removal should win.
//!
//! Run with `cargo run --example document_blocks`.

use std::collections::BTreeMap;

use tpt_version::error::ObjectError;
use tpt_version::{
    ActorId, Entity, EntityId, LogicalClock, MemoryStore, RepoError, Repository, Resolution,
    Scalar, UirNode,
};

/// A paragraph or heading. The id is assigned once and never reused.
struct Block {
    id: EntityId,
    text: String,
    tags: Vec<String>,
}

impl Entity for Block {
    fn id(&self) -> EntityId {
        self.id
    }

    fn to_uir(&self) -> UirNode {
        UirNode::Map(BTreeMap::from([
            (
                "id".into(),
                UirNode::Scalar(Scalar::Bytes(self.id.as_bytes().to_vec())),
            ),
            ("text".into(), UirNode::from(self.text.as_str())),
            (
                "tags".into(),
                UirNode::List(self.tags.iter().map(|tag| tag.as_str().into()).collect()),
            ),
        ]))
    }

    fn from_uir(node: &UirNode) -> Result<Self, ObjectError> {
        let UirNode::Map(fields) = node else {
            return Err(ObjectError::InvalidValue("block"));
        };
        let Some(UirNode::Scalar(Scalar::Bytes(id))) = fields.get("id") else {
            return Err(ObjectError::InvalidValue("block id"));
        };
        let Some(UirNode::Scalar(Scalar::Text(text))) = fields.get("text") else {
            return Err(ObjectError::InvalidValue("block text"));
        };
        let Some(UirNode::List(items)) = fields.get("tags") else {
            return Err(ObjectError::InvalidValue("block tags"));
        };
        let tags = items
            .iter()
            .map(|item| match item {
                UirNode::Scalar(Scalar::Text(tag)) => Ok(tag.clone()),
                _ => Err(ObjectError::InvalidValue("tag")),
            })
            .collect::<Result<_, _>>()?;
        let id = <[u8; 16]>::try_from(id.as_slice())
            .map_err(|_| ObjectError::InvalidValue("block id length"))?;
        Ok(Self {
            id: EntityId::from_bytes(id),
            text: text.clone(),
            tags,
        })
    }
}

fn block(n: u8, text: &str, tags: &[&str]) -> Block {
    Block {
        id: EntityId::from_bytes([n; 16]),
        text: text.into(),
        tags: tags.iter().map(|tag| (*tag).into()).collect(),
    }
}

fn main() -> Result<(), RepoError> {
    let ana = ActorId::from_bytes([1; 16]);
    let ben = ActorId::from_bytes([2; 16]);
    let mut repo = Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main")?;

    repo.put_entity(&block(1, "Introduction", &[]))?;
    repo.put_entity(&block(2, "Results pending.", &[]))?;
    let base = repo.commit("first draft", &ana)?;
    repo.branch("ben", base)?;

    // Ana fills in the results on main. Ben tags the introduction on his branch.
    repo.put_entity(&block(2, "Results: 14% faster.", &[]))?;
    repo.commit("ana: results", &ana)?;
    repo.switch_branch("ben")?;
    repo.put_entity(&block(1, "Introduction", &["needs-review"]))?;
    repo.commit("ben: flag intro", &ben)?;

    // Different blocks, so no conflict.
    repo.switch_branch("main")?;
    let merged = repo.merge("ben")?;
    println!("merge: {} conflict(s)", merged.conflicts.len());
    repo.commit("merge ben", &ana)?;

    // Both sides now change the tags of block 1 from the same merge commit.
    // That is a conflict on `tags`.
    let merge_commit = repo.head().expect("merged");
    repo.branch("ben-tags", merge_commit)?;
    repo.put_entity(&block(1, "Introduction", &["draft"]))?;
    repo.commit("ana: tag draft", &ana)?;
    repo.switch_branch("ben-tags")?;
    repo.put_entity(&block(1, "Introduction", &["approved"]))?;
    repo.commit("ben: tag approved", &ben)?;
    repo.switch_branch("main")?;

    let result = repo.merge("ben-tags")?;
    for conflict in &result.conflicts {
        for field in &conflict.fields {
            println!("conflict on {}", field.path.join("."));
        }
    }
    repo.resolve_conflict(&EntityId::from_bytes([1; 16]), Resolution::Ours)?;
    repo.commit("merge ben, keep ana's tags", &ana)?;

    let intro: Block = repo
        .entity(&EntityId::from_bytes([1; 16]))?
        .expect("introduction is staged");
    println!("{} tags: {:?}", intro.text, intro.tags);
    Ok(())
}
