//! An application skeleton on top of tpt-version. It shows the three parts most
//! apps need:
//!
//! - an entity, derived with `#[derive(Entity)]`;
//! - a workspace type that owns the repository and fixes the author;
//! - a merge UI, as a trait, that the app implements to settle conflicts.
//!
//! The `MergeUi` implementation is the part that changes for a real app. It would
//! show the two versions to a person. `PreferIncoming` picks the incoming side of
//! every conflict, so the example runs unattended.
//!
//! Run with `cargo run --example app_template`.

use tpt_version::Conflict;
use tpt_version::prelude::*;

/// A note in the app's document.
#[derive(Entity, Debug, Clone, PartialEq)]
pub struct Note {
    #[entity(id)]
    id: EntityId,
    pub title: String,
    pub body: String,
}

impl Note {
    pub fn new(n: u8, title: &str, body: &str) -> Self {
        Self {
            id: EntityId::from_bytes([n; 16]),
            title: title.into(),
            body: body.into(),
        }
    }
}

/// The app's merge UI. Given one conflict, it returns how to settle it.
pub trait MergeUi {
    fn resolve(&mut self, conflict: &Conflict) -> Resolution;
}

/// Picks the incoming side of every conflict. A real UI would ask a person.
#[derive(Default)]
pub struct PreferIncoming;

impl MergeUi for PreferIncoming {
    fn resolve(&mut self, _conflict: &Conflict) -> Resolution {
        Resolution::Theirs
    }
}

/// The app's document. It owns the repository and records every commit under one
/// author, so call sites never pass identities around.
pub struct Workspace<UI: MergeUi> {
    repo: Repository<MemoryStore, LogicalClock>,
    author: ActorId,
    ui: UI,
}

impl<UI: MergeUi> Workspace<UI> {
    pub fn new(author: ActorId, ui: UI) -> Result<Self, RepoError> {
        let repo = Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main")?;
        Ok(Self { repo, author, ui })
    }

    pub fn save(&mut self, note: &Note) -> Result<(), RepoError> {
        self.repo.put_entity(note)
    }

    pub fn record(&mut self, message: &str) -> Result<ContentHash, RepoError> {
        self.repo.commit(message, &self.author)
    }

    /// Merges `branch` and settles every conflict through the UI. Returns how
    /// many conflicts were settled. The caller commits the merge afterwards.
    pub fn merge(&mut self, branch: &str) -> Result<usize, RepoError> {
        let result = self.repo.merge(branch)?;
        for conflict in &result.conflicts {
            let choice = self.ui.resolve(conflict);
            self.repo.resolve_conflict(&conflict.entity, choice)?;
        }
        Ok(result.conflicts.len())
    }

    pub fn note(&self, n: u8) -> Result<Option<Note>, RepoError> {
        self.repo.entity(&EntityId::from_bytes([n; 16]))
    }

    pub fn branch_at_head(&mut self, name: &str) -> Result<(), RepoError> {
        let head = self.repo.head().expect("the workspace has a commit");
        self.repo.branch(name, head)
    }

    pub fn switch(&mut self, name: &str) -> Result<(), RepoError> {
        self.repo.switch_branch(name)
    }
}

fn main() -> Result<(), RepoError> {
    let author = ActorId::from_bytes([1; 16]);
    let mut doc = Workspace::new(author, PreferIncoming)?;

    doc.save(&Note::new(1, "Intro", "Draft."))?;
    doc.record("first draft")?;
    doc.branch_at_head("review")?;

    doc.save(&Note::new(1, "Intro", "Draft, reworded on main."))?;
    doc.record("main: reword")?;

    doc.switch("review")?;
    doc.save(&Note::new(1, "Introduction", "Draft, reworded in review."))?;
    doc.record("review: retitle")?;
    doc.switch("main")?;

    let settled = doc.merge("review")?;
    doc.record("merge review")?;
    println!("settled {settled} conflict(s)");

    let intro = doc.note(1)?.expect("the note exists");
    println!("{}: {}", intro.title, intro.body);
    Ok(())
}
