//! A key-value document with branches, merges and history, for the browser.
//!
//! The core is `no_std`, so this runs in memory. Each page load starts empty.
//! The JavaScript side calls these methods directly, and every method returns
//! a string or a count, so no types cross the boundary.

use std::fmt::Display;

use tpt_version::prelude::*;
use wasm_bindgen::prelude::*;

/// One key-value pair. The id comes from the key, so it survives value edits.
#[derive(Entity, Debug, Clone, PartialEq)]
struct Entry {
    #[entity(id)]
    id: EntityId,
    key: String,
    value: String,
}

fn entry(key: &str, value: &str) -> Entry {
    let hash = ContentHash::of(key.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash.as_bytes()[..16]);
    Entry {
        id: EntityId::from_bytes(bytes),
        key: key.into(),
        value: value.into(),
    }
}

fn js<E: Display>(error: E) -> JsValue {
    JsValue::from_str(&error.to_string())
}

const AUTHOR: ActorId = ActorId::from_bytes([0xD0; 16]);

/// A document under version control.
#[wasm_bindgen]
pub struct Doc {
    repo: Repository<MemoryStore, LogicalClock>,
    branch: String,
}

#[wasm_bindgen]
impl Doc {
    /// An empty document on `main`.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Doc, JsValue> {
        let branch = tpt_version::repo::DEFAULT_BRANCH.to_string();
        let repo = Repository::open(
            MemoryStore::default(),
            LogicalClock::starting_at(1),
            &branch,
        )
        .map_err(js)?;
        Ok(Doc { repo, branch })
    }

    /// Stages a key. Nothing is recorded until `commit`.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), JsValue> {
        self.repo.put_entity(&entry(key, value)).map_err(js)
    }

    /// Records the staged changes on the current branch. Returns the commit hash.
    pub fn commit(&mut self, message: &str) -> Result<String, JsValue> {
        self.repo
            .commit(message, &AUTHOR)
            .map(|hash| hash.to_string())
            .map_err(js)
    }

    /// Creates a branch at the head.
    pub fn branch(&mut self, name: &str) -> Result<(), JsValue> {
        let head = self
            .repo
            .head()
            .ok_or_else(|| js("nothing committed yet"))?;
        self.repo.branch(name, head).map_err(js)
    }

    /// Checks out a branch.
    pub fn switch(&mut self, name: &str) -> Result<(), JsValue> {
        self.repo.switch_branch(name).map_err(js)?;
        self.branch = name.to_string();
        Ok(())
    }

    /// Merges `source` into the current branch. Every conflict is settled in
    /// favour of `source` when `prefer_source` is true, and of the current branch
    /// otherwise. Records the merge and returns its hash.
    pub fn merge(
        &mut self,
        source: &str,
        message: &str,
        prefer_source: bool,
    ) -> Result<String, JsValue> {
        let result = self.repo.merge(source).map_err(js)?;
        let choice = if prefer_source {
            Resolution::Theirs
        } else {
            Resolution::Ours
        };
        for conflict in &result.conflicts {
            self.repo
                .resolve_conflict(&conflict.entity, choice.clone())
                .map_err(js)?;
        }
        self.repo
            .commit(message, &AUTHOR)
            .map(|hash| hash.to_string())
            .map_err(js)
    }

    /// The messages on the current branch, newest first, one per line.
    pub fn log(&self) -> Result<String, JsValue> {
        let commits = self.repo.log(&self.branch).map_err(js)?;
        Ok(commits
            .iter()
            .map(|commit| commit.message.as_str())
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// The branches, one per line. The current one is marked with `*`.
    pub fn branches(&self) -> Result<String, JsValue> {
        let names = self.repo.list_branches().map_err(js)?;
        Ok(names
            .iter()
            .map(|name| {
                let marker = if *name == self.branch { "*" } else { " " };
                format!("{marker} {name}")
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// The key-value pairs at the head, sorted by key, one `key=value` per line.
    pub fn list(&self) -> Result<String, JsValue> {
        let tree = self.repo.head_tree().map_err(js)?;
        let mut lines = Vec::new();
        for id in tree.entries.keys() {
            if let Some(found) = self.repo.entity::<Entry>(id).map_err(js)? {
                lines.push((found.key, found.value));
            }
        }
        lines.sort();
        Ok(lines
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}
