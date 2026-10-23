//! Working-copy diffs, and the commit and merge hooks.

mod common;

use std::sync::{Arc, Mutex};

use common::{author, id, new_repo, vertex};
use tpt_version::RepoError;
use tpt_version::diff::DiffEntry;
use tpt_version::merge::MergeResult;
use tpt_version::repo::Hooks;

/// Records what the hooks saw, and optionally refuses commits that remove
/// entities.
struct Recorder {
    events: Arc<Mutex<Vec<String>>>,
    refuse_removals: bool,
}

impl Hooks for Recorder {
    fn pre_commit(&mut self, changes: &[DiffEntry]) -> Result<(), String> {
        self.events
            .lock()
            .unwrap()
            .push(format!("pre:{}", changes.len()));
        let removes = changes
            .iter()
            .any(|change| matches!(change, DiffEntry::Removed { .. }));
        if self.refuse_removals && removes {
            return Err("removals are not allowed".into());
        }
        Ok(())
    }

    fn post_merge(&mut self, branch: &str, result: &MergeResult) {
        self.events
            .lock()
            .unwrap()
            .push(format!("merge:{branch}:{}", result.conflicts.len()));
    }
}

#[test]
fn diff_working_reports_uncommitted_changes() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    repo.commit("one", &author()).unwrap();
    assert!(repo.diff_working().unwrap().is_empty());

    repo.put_entity(&vertex(1, [1.0; 3])).unwrap();
    repo.put_entity(&vertex(2, [0.0; 3])).unwrap();
    let changes = repo.diff_working().unwrap();
    assert_eq!(changes.len(), 2);
    assert!(
        changes
            .iter()
            .any(|c| matches!(c, DiffEntry::Modified { id: i, .. } if *i == id(1)))
    );
    assert!(
        changes
            .iter()
            .any(|c| matches!(c, DiffEntry::Added { id: i, .. } if *i == id(2)))
    );
}

#[test]
fn pre_commit_sees_the_changes_and_can_refuse_them() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut repo = new_repo().with_hooks(Box::new(Recorder {
        events: Arc::clone(&events),
        refuse_removals: true,
    }));
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    repo.put_entity(&vertex(2, [0.0; 3])).unwrap();
    let first = repo.commit("two", &author()).unwrap();

    repo.remove_entity(&id(2)).unwrap();
    let refused = repo.commit("drop two", &author());
    assert_eq!(
        refused,
        Err(RepoError::HookRejected("removals are not allowed".into()))
    );
    assert_eq!(repo.head(), Some(first), "a refused commit moves nothing");

    // A refused commit leaves its changes staged, so put entity 2 back first.
    repo.put_entity(&vertex(2, [0.0; 3])).unwrap();
    // Changing an entity is still allowed.
    repo.put_entity(&vertex(1, [9.0; 3])).unwrap();
    repo.commit("move one", &author()).unwrap();
    assert_eq!(*events.lock().unwrap(), ["pre:2", "pre:1", "pre:1"]);
}

#[test]
fn post_merge_reports_the_branch_and_its_conflicts() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut repo = new_repo().with_hooks(Box::new(Recorder {
        events: Arc::clone(&events),
        refuse_removals: false,
    }));
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let base = repo.commit("base", &author()).unwrap();
    repo.branch("side", base).unwrap();

    repo.switch_branch("side").unwrap();
    repo.put_entity(&vertex(2, [1.0; 3])).unwrap();
    repo.commit("side work", &author()).unwrap();
    repo.switch_branch("main").unwrap();
    repo.put_entity(&vertex(3, [1.0; 3])).unwrap();
    repo.commit("main work", &author()).unwrap();

    repo.merge("side").unwrap();
    assert_eq!(events.lock().unwrap().last().unwrap(), "merge:side:0");
}
