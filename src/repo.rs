//! The repository: a staged working copy over a commit DAG, with branches,
//! merges and optionally signed commits.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::Cell;

use crate::dag;
use crate::entity::Entity;
use crate::error::RepoError;
use crate::merge::{self, MergeResult, Resolution};
use crate::object::{ActorId, Commit, Conflict, ContentHash, EntityId, Object, Tree};
use crate::sign::{Signature, Signer};
use crate::store::{CasStore, RefStore, load_entity, load_tree, store_object};

/// The branch a fresh repository starts on.
pub const DEFAULT_BRANCH: &str = "main";

/// A source of commit timestamps. Injected so histories can be reproduced.
pub trait Clock {
    fn now(&self) -> u64;
}

/// A clock that returns successive values from a starting point.
#[derive(Debug, Default)]
pub struct LogicalClock {
    next: Cell<u64>,
}

impl LogicalClock {
    pub fn starting_at(value: u64) -> Self {
        Self {
            next: Cell::new(value),
        }
    }
}

impl Clock for LogicalClock {
    fn now(&self) -> u64 {
        let now = self.next.get();
        self.next.set(now + 1);
        now
    }
}

/// Unix seconds from the system clock.
#[cfg(feature = "std")]
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

#[cfg(feature = "std")]
impl Clock for SystemClock {
    fn now(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs())
    }
}

/// Checks a branch name: ASCII letters, digits, `.`, `_`, `-` and `/`, with no
/// empty, leading, trailing or doubled separators.
pub fn validate_branch_name(name: &str) -> Result<(), RepoError> {
    let valid = !name.is_empty()
        && name.len() <= 200
        && !name.starts_with(['/', '.', '-'])
        && !name.ends_with(['/', '.'])
        && !name.contains("..")
        && !name.contains("//")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'));
    if valid {
        Ok(())
    } else {
        Err(RepoError::InvalidBranchName(name.into()))
    }
}

/// A versioned collection of entities.
///
/// Edits go to a staged working copy. [`Repository::commit`] snapshots it into
/// the DAG. A merge that leaves conflicts blocks commits until each conflict is
/// resolved with [`Repository::resolve_conflict`].
pub struct Repository<S, C> {
    store: S,
    clock: C,
    signer: Option<Box<dyn Signer>>,
    staged: BTreeMap<EntityId, ContentHash>,
    head: Option<ContentHash>,
    branch: Option<String>,
    merge_parents: Vec<ContentHash>,
    conflicts: Vec<Conflict>,
}

impl<S: CasStore + RefStore, C: Clock> Repository<S, C> {
    /// Opens the repository on `branch`. If the branch exists, the working copy
    /// starts at its head. Otherwise the branch is unborn and the first commit
    /// creates it.
    pub fn open(store: S, clock: C, branch: &str) -> Result<Self, RepoError> {
        validate_branch_name(branch)?;
        let head = store.read_ref(branch)?;
        let staged = match head {
            Some(commit) => load_tree(&store, &dag::commit_tree(&store, &commit)?)?.entries,
            None => BTreeMap::new(),
        };
        Ok(Self {
            store,
            clock,
            signer: None,
            staged,
            head,
            branch: Some(branch.into()),
            merge_parents: Vec::new(),
            conflicts: Vec::new(),
        })
    }

    /// Signs every commit made from now on.
    pub fn with_signer(mut self, signer: Box<dyn Signer>) -> Self {
        self.signer = Some(signer);
        self
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    /// The commit the working copy is based on.
    pub fn head(&self) -> Option<ContentHash> {
        self.head
    }

    /// The checked-out branch, or `None` when detached.
    pub fn current_branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    /// Conflicts from the last merge that are not resolved yet.
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// Stages an entity, replacing any staged version.
    pub fn put_entity<E: Entity>(&mut self, entity: &E) -> Result<(), RepoError> {
        let id = entity.id();
        self.ensure_not_conflicted(&id)?;
        let hash = store_object(&mut self.store, &Object::Entity(entity.to_uir()))?;
        self.staged.insert(id, hash);
        Ok(())
    }

    /// Unstages an entity. Returns whether it was staged.
    pub fn remove_entity(&mut self, id: &EntityId) -> Result<bool, RepoError> {
        self.ensure_not_conflicted(id)?;
        Ok(self.staged.remove(id).is_some())
    }

    /// Reads a staged entity.
    pub fn entity<E: Entity>(&self, id: &EntityId) -> Result<Option<E>, RepoError> {
        let Some(hash) = self.staged.get(id) else {
            return Ok(None);
        };
        let node = load_entity(&self.store, hash)?;
        Ok(Some(E::from_uir(&node)?))
    }

    /// Snapshots the working copy into a commit and advances the current branch.
    pub fn commit(&mut self, message: &str, author: &ActorId) -> Result<ContentHash, RepoError> {
        if !self.conflicts.is_empty() {
            return Err(RepoError::UnresolvedConflicts(self.conflicts.len()));
        }
        let tree_hash = store_object(
            &mut self.store,
            &Object::Tree(Tree {
                entries: self.staged.clone(),
            }),
        )?;
        let mut parents: Vec<ContentHash> = self.head.into_iter().collect();
        parents.append(&mut self.merge_parents);

        let mut commit = Commit {
            tree_hash,
            parents,
            author: *author,
            timestamp: self.clock.now(),
            message: message.into(),
            signature: None,
        };
        if let Some(signer) = &self.signer {
            commit.signature = Some(Signature {
                signer: signer.public_key(),
                bytes: signer.sign(&commit.signing_bytes()),
            });
        }

        let hash = store_object(&mut self.store, &Object::Commit(commit))?;
        if let Some(name) = &self.branch {
            self.store.write_ref(name, hash)?;
        }
        self.head = Some(hash);
        Ok(hash)
    }

    /// Creates a branch pointing at `commit`.
    pub fn branch(&mut self, name: &str, commit: ContentHash) -> Result<(), RepoError> {
        validate_branch_name(name)?;
        if self.store.read_ref(name)?.is_some() {
            return Err(RepoError::BranchExists(name.into()));
        }
        dag::load_commit(&self.store, &commit)?;
        self.store.write_ref(name, commit)?;
        Ok(())
    }

    /// The commit a branch points at, or `None` if it does not exist.
    pub fn resolve_branch(&self, name: &str) -> Result<Option<ContentHash>, RepoError> {
        validate_branch_name(name)?;
        Ok(self.store.read_ref(name)?)
    }

    pub fn list_branches(&self) -> Result<Vec<String>, RepoError> {
        Ok(self.store.list_refs()?)
    }

    /// Deletes a branch. The checked-out branch cannot be deleted.
    pub fn delete_branch(&mut self, name: &str) -> Result<(), RepoError> {
        if self.branch.as_deref() == Some(name) {
            return Err(RepoError::CannotDeleteCurrentBranch(name.into()));
        }
        if self.store.delete_ref(name)? {
            Ok(())
        } else {
            Err(RepoError::BranchNotFound(name.into()))
        }
    }

    /// Moves the working copy to a commit, detached from any branch.
    pub fn checkout(&mut self, commit: ContentHash) -> Result<(), RepoError> {
        self.ensure_clean()?;
        self.staged = commit_entries(&self.store, &commit)?;
        self.head = Some(commit);
        self.branch = None;
        Ok(())
    }

    /// Checks out a branch.
    pub fn switch_branch(&mut self, name: &str) -> Result<(), RepoError> {
        let target = self
            .resolve_branch(name)?
            .ok_or_else(|| RepoError::BranchNotFound(name.into()))?;
        self.ensure_clean()?;
        self.staged = commit_entries(&self.store, &target)?;
        self.head = Some(target);
        self.branch = Some(name.into());
        Ok(())
    }

    /// The history reachable from a branch, newest first.
    pub fn log(&self, branch: &str) -> Result<Vec<Commit>, RepoError> {
        let head = self
            .resolve_branch(branch)?
            .ok_or_else(|| RepoError::BranchNotFound(branch.into()))?;
        dag::ancestors(&self.store, &[head])?
            .iter()
            .map(|hash| dag::load_commit(&self.store, hash))
            .collect()
    }

    /// Merges a branch into the current one.
    ///
    /// Fast-forwards when possible. Otherwise merges three-way and stages the
    /// result. The merge is recorded by the next [`Repository::commit`], and
    /// that commit is refused while conflicts remain.
    pub fn merge(&mut self, branch: &str) -> Result<MergeResult, RepoError> {
        self.ensure_clean()?;
        let theirs = self
            .resolve_branch(branch)?
            .ok_or_else(|| RepoError::BranchNotFound(branch.into()))?;
        let theirs_tree = dag::commit_tree(&self.store, &theirs)?;

        let Some(ours) = self.head else {
            // Unborn branch: adopt the other branch's history.
            self.move_to(theirs)?;
            return Ok(MergeResult {
                merged_tree: theirs_tree,
                conflicts: Vec::new(),
            });
        };
        let ours_tree = dag::commit_tree(&self.store, &ours)?;

        if ours == theirs || dag::is_ancestor(&self.store, &theirs, &ours)? {
            return Ok(MergeResult {
                merged_tree: ours_tree,
                conflicts: Vec::new(),
            });
        }
        if dag::is_ancestor(&self.store, &ours, &theirs)? {
            self.move_to(theirs)?;
            return Ok(MergeResult {
                merged_tree: theirs_tree,
                conflicts: Vec::new(),
            });
        }

        let base_tree = match dag::find_merge_base(&self.store, &ours, &theirs)? {
            Some(base) => dag::commit_tree(&self.store, &base)?,
            None => store_object(&mut self.store, &Object::Tree(Tree::default()))?,
        };
        let result = merge::merge_trees(&mut self.store, base_tree, ours_tree, theirs_tree)?;
        self.staged = load_tree(&self.store, &result.merged_tree)?.entries;
        self.merge_parents = vec![theirs];
        self.conflicts = result.conflicts.clone();
        Ok(result)
    }

    /// Resolves the pending conflict on `id`.
    pub fn resolve_conflict(
        &mut self,
        id: &EntityId,
        resolution: Resolution,
    ) -> Result<(), RepoError> {
        let index = self
            .conflicts
            .iter()
            .position(|conflict| conflict.entity == *id)
            .ok_or(RepoError::NoConflict(*id))?;
        let chosen = match resolution {
            Resolution::Ours => self.conflicts[index].ours,
            Resolution::Theirs => self.conflicts[index].theirs,
            Resolution::Custom(node) => Some(store_object(&mut self.store, &Object::Entity(node))?),
        };
        self.conflicts.remove(index);
        match chosen {
            Some(hash) => {
                self.staged.insert(*id, hash);
            }
            None => {
                self.staged.remove(id);
            }
        }
        Ok(())
    }

    fn move_to(&mut self, target: ContentHash) -> Result<(), RepoError> {
        self.staged = commit_entries(&self.store, &target)?;
        self.head = Some(target);
        self.merge_parents.clear();
        if let Some(name) = &self.branch {
            self.store.write_ref(name, target)?;
        }
        Ok(())
    }

    fn head_entries(&self) -> Result<BTreeMap<EntityId, ContentHash>, RepoError> {
        match self.head {
            Some(commit) => commit_entries(&self.store, &commit),
            None => Ok(BTreeMap::new()),
        }
    }

    /// Refuses operations that would discard uncommitted work or a pending merge.
    fn ensure_clean(&self) -> Result<(), RepoError> {
        if !self.conflicts.is_empty() {
            return Err(RepoError::UnresolvedConflicts(self.conflicts.len()));
        }
        if !self.merge_parents.is_empty() || self.staged != self.head_entries()? {
            return Err(RepoError::DirtyWorkingCopy);
        }
        Ok(())
    }

    fn ensure_not_conflicted(&self, id: &EntityId) -> Result<(), RepoError> {
        if self.conflicts.iter().any(|conflict| conflict.entity == *id) {
            Err(RepoError::ConflictPending(*id))
        } else {
            Ok(())
        }
    }
}

fn commit_entries<S: CasStore>(
    store: &S,
    commit: &ContentHash,
) -> Result<BTreeMap<EntityId, ContentHash>, RepoError> {
    Ok(load_tree(store, &dag::commit_tree(store, commit)?)?.entries)
}
