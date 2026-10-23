//! The repository: a staged working copy over a commit DAG, with branches,
//! merges and optionally signed commits.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::Cell;

use crate::dag;
use crate::diff::{DiffEntry, diff_trees};
use crate::entity::Entity;
use crate::error::RepoError;
use crate::history::{self, EntityVersion, FieldBlame};
use crate::maintenance::{self, GcReport};
use crate::merge::{self, MergeResult, Resolution};
use crate::object::{ActorId, Commit, Conflict, ContentHash, EntityId, Object, Tree};
use crate::sign::{Signature, Signer};
use crate::store::{CasStore, PruneStore, RefStore, load_entity, load_tree, store_object};
use crate::trust::TrustPolicy;
use crate::uir::UirNode;

/// The branch a fresh repository starts on.
pub const DEFAULT_BRANCH: &str = "main";

/// A source of commit timestamps. Injected so histories can be reproduced.
pub trait Clock {
    /// The timestamp for the next commit.
    fn now(&self) -> u64;
}

/// A clock that returns successive values from a starting point.
#[derive(Debug, Default)]
pub struct LogicalClock {
    next: Cell<u64>,
}

impl LogicalClock {
    /// A clock whose first call to [`Clock::now`] returns `value`.
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

/// Refs under this prefix are tags. Branch names may not start with it.
const TAG_PREFIX: &str = "tags/";

/// The ref that holds tag `name`.
fn tag_ref(name: &str) -> String {
    alloc::format!("{TAG_PREFIX}{name}")
}

/// Checks a branch name: ASCII letters, digits, `.`, `_`, `-` and `/`, with no
/// empty, leading, trailing or doubled separators. Each `/`-separated part must
/// also be usable as a file name on every supported platform: it may not end
/// in `.lock`, and it may not be a Windows device name such as `CON` or `NUL`.
pub fn validate_branch_name(name: &str) -> Result<(), RepoError> {
    let valid = !name.is_empty()
        && name.len() <= 200
        && !name.starts_with(TAG_PREFIX)
        && !name.starts_with(['/', '.', '-'])
        && !name.ends_with(['/', '.'])
        && !name.contains("..")
        && !name.contains("//")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
        && name
            .split('/')
            .all(|part| !part.ends_with(".lock") && !is_windows_device_name(part));
    if valid {
        Ok(())
    } else {
        Err(RepoError::InvalidBranchName(name.into()))
    }
}

/// Windows reserves these names in every directory, with or without an
/// extension (`CON.txt` is still `CON`), and ignores case.
fn is_windows_device_name(part: &str) -> bool {
    const RESERVED: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];
    let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
    RESERVED.contains(&stem.as_str())
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

/// Callbacks around commits and merges. Both methods do nothing by default, so
/// an implementation overrides only what it needs.
pub trait Hooks {
    /// Runs before a commit is written, with the changes it would record. An
    /// error refuses the commit and is returned as [`RepoError::HookRejected`].
    fn pre_commit(&mut self, changes: &[DiffEntry]) -> Result<(), String> {
        let _ = changes;
        Ok(())
    }

    /// Runs after a merge of `branch` has been applied to the working copy.
    fn post_merge(&mut self, branch: &str, result: &MergeResult) {
        let _ = (branch, result);
    }
}

/// A snapshot of the working copy, from [`Repository::status`].
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    /// The checked-out branch, or `None` when detached.
    pub branch: Option<String>,
    /// The commit the working copy is based on.
    pub head: Option<ContentHash>,
    /// Changes a commit would record.
    pub changes: Vec<DiffEntry>,
    /// Entities with an unresolved merge conflict.
    pub conflicts: Vec<EntityId>,
    /// Whether a merge is staged and waiting for its commit.
    pub merging: bool,
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
    hooks: Option<Box<dyn Hooks>>,
    trust: Option<TrustPolicy>,
}

impl<S: CasStore + RefStore, C: Clock> Repository<S, C> {
    /// Opens the repository on `branch`. If the branch exists, the working copy
    /// starts at its head. Otherwise the branch is unborn and the first commit
    /// creates it.
    pub fn open(store: S, clock: C, branch: &str) -> Result<Self, RepoError> {
        validate_branch_name(branch)?;
        let head = store.read_ref(branch)?;
        if head.is_none() {
            // The first commit on an unborn branch creates it, so it must not
            // clash with an existing branch either.
            ensure_no_name_clash(&store, branch)?;
        }
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
            hooks: None,
            trust: None,
        })
    }

    /// Signs every commit made from now on.
    pub fn with_signer(mut self, signer: Box<dyn Signer>) -> Self {
        self.signer = Some(signer);
        self
    }

    /// Installs hooks that run around every commit and merge from now on.
    pub fn with_hooks(mut self, hooks: Box<dyn Hooks>) -> Self {
        self.hooks = Some(hooks);
        self
    }

    /// Applies a trust policy from now on. Commits to a governed branch must
    /// then be signed by a trusted key, and so must every commit a merge brings
    /// into one. Commits made before the policy are not re-checked.
    pub fn with_trust_policy(mut self, policy: TrustPolicy) -> Self {
        self.trust = Some(policy);
        self
    }

    /// Refuses a merge into a governed branch when it brings in a commit that
    /// no trusted key signed.
    fn ensure_trusted_incoming(&self, theirs: &ContentHash) -> Result<(), RepoError> {
        let (Some(policy), Some(branch)) = (&self.trust, &self.branch) else {
            return Ok(());
        };
        if !policy.governs(branch) {
            return Ok(());
        }
        let ours: BTreeSet<ContentHash> = match self.head {
            Some(head) => dag::ancestors(&self.store, &[head])?.into_iter().collect(),
            None => BTreeSet::new(),
        };
        for hash in dag::ancestors(&self.store, &[*theirs])? {
            if !ours.contains(&hash) && !policy.accepts(&dag::load_commit(&self.store, &hash)?) {
                return Err(RepoError::Untrusted(branch.clone()));
            }
        }
        Ok(())
    }

    /// The changes in the working copy that a commit would record, compared
    /// with the head. Empty when the working copy matches the head.
    /// The commits on the current branch that changed `id`, newest first. Follows
    /// the first parent of each commit.
    pub fn entity_history(&self, id: &EntityId) -> Result<Vec<EntityVersion>, RepoError> {
        match self.head {
            Some(head) => history::entity_history(&self.store, head, id),
            None => Ok(Vec::new()),
        }
    }

    /// For each field of entity `id` at the head, the commit that set its
    /// current value. Empty if the entity is absent from the head.
    pub fn field_blame(&self, id: &EntityId) -> Result<Vec<FieldBlame>, RepoError> {
        match self.head {
            Some(head) => history::field_blame(&self.store, head, id),
            None => Ok(Vec::new()),
        }
    }

    /// A summary of the working copy: where it is, what would be committed, and
    /// what still blocks a commit.
    pub fn status(&self) -> Result<Status, RepoError> {
        Ok(Status {
            branch: self.branch.clone(),
            head: self.head,
            changes: self.diff_working()?,
            conflicts: self
                .conflicts
                .iter()
                .map(|conflict| conflict.entity)
                .collect(),
            merging: !self.merge_parents.is_empty(),
        })
    }

    /// The changes in the working copy that a commit would record, compared
    /// with the head. Empty when the working copy matches the head.
    pub fn diff_working(&self) -> Result<Vec<DiffEntry>, RepoError> {
        let old = Tree {
            entries: self.head_entries()?,
        };
        let new = Tree {
            entries: self.staged.clone(),
        };
        diff_trees(&self.store, &old, &new)
    }

    /// The underlying store, for reading objects directly.
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
    ///
    /// The stored form must read back as an entity with the same id, and it
    /// must be within the nesting depth that decoding accepts. Otherwise the
    /// entity could be stored but never loaded, or loaded under another id.
    pub fn put_entity<E: Entity>(&mut self, entity: &E) -> Result<(), RepoError> {
        let id = entity.id();
        self.ensure_not_conflicted(&id)?;
        let node = entity.to_uir();
        check_storable::<E>(&node, &id)?;
        let hash = store_object(&mut self.store, &Object::Entity(node))?;
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
    ///
    /// Two cases are allowed on purpose:
    /// - A commit with no changes is recorded anyway, as Git does, so a message
    ///   or a merge parent is never lost.
    /// - With no branch checked out (after [`Repository::checkout`]), the commit
    ///   moves no ref. It is reachable only through its hash, so call
    ///   [`Repository::branch`] on the returned hash to keep it.
    pub fn commit(&mut self, message: &str, author: &ActorId) -> Result<ContentHash, RepoError> {
        let parents = self
            .head
            .into_iter()
            .chain(self.merge_parents.iter().copied())
            .collect();
        self.commit_with_parents(message, author, parents)
    }

    /// Replaces the head commit with one that records the working copy, keeping
    /// the head's parents. The old head stays in the store until [`Repository::gc`]
    /// removes it. Refused while a merge is staged.
    pub fn amend(&mut self, message: &str, author: &ActorId) -> Result<ContentHash, RepoError> {
        if !self.merge_parents.is_empty() {
            return Err(RepoError::MergeInProgress);
        }
        let head = self.head.ok_or(RepoError::NothingToAmend)?;
        let parents = dag::load_commit(&self.store, &head)?.parents;
        self.commit_with_parents(message, author, parents)
    }

    fn commit_with_parents(
        &mut self,
        message: &str,
        author: &ActorId,
        parents: Vec<ContentHash>,
    ) -> Result<ContentHash, RepoError> {
        if !self.conflicts.is_empty() {
            return Err(RepoError::UnresolvedConflicts(self.conflicts.len()));
        }
        if self.hooks.is_some() {
            let changes = self.diff_working()?;
            if let Some(hooks) = self.hooks.as_mut() {
                hooks
                    .pre_commit(&changes)
                    .map_err(RepoError::HookRejected)?;
            }
        }
        let tree_hash = store_object(
            &mut self.store,
            &Object::Tree(Tree {
                entries: self.staged.clone(),
            }),
        )?;
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

        if let (Some(policy), Some(name)) = (&self.trust, &self.branch)
            && policy.governs(name)
            && !policy.accepts(&commit)
        {
            return Err(RepoError::Untrusted(name.clone()));
        }

        let hash = store_object(&mut self.store, &Object::Commit(commit))?;
        if let Some(name) = self.branch.clone() {
            self.advance_branch(&name, self.head, hash)?;
        }
        self.head = Some(hash);
        self.merge_parents.clear();
        Ok(hash)
    }

    /// Creates a branch pointing at `commit`.
    pub fn branch(&mut self, name: &str, commit: ContentHash) -> Result<(), RepoError> {
        validate_branch_name(name)?;
        if self.store.read_ref(name)?.is_some() {
            return Err(RepoError::BranchExists(name.into()));
        }
        ensure_no_name_clash(&self.store, name)?;
        dag::load_commit(&self.store, &commit)?;
        // Creating the branch must not overwrite one that appeared since the
        // check above.
        if !self.store.compare_and_swap_ref(name, None, commit)? {
            return Err(RepoError::BranchExists(name.into()));
        }
        Ok(())
    }

    /// The commit a branch points at, or `None` if it does not exist.
    pub fn resolve_branch(&self, name: &str) -> Result<Option<ContentHash>, RepoError> {
        validate_branch_name(name)?;
        Ok(self.store.read_ref(name)?)
    }

    /// All branch names, sorted. Tags are not branches and are not listed.
    pub fn list_branches(&self) -> Result<Vec<String>, RepoError> {
        Ok(self
            .store
            .list_refs()?
            .into_iter()
            .filter(|name| !name.starts_with(TAG_PREFIX))
            .collect())
    }

    /// Creates an immutable tag pointing at `commit`. A tag name follows the
    /// branch name rules. Tags are never moved by a commit.
    pub fn create_tag(&mut self, name: &str, commit: ContentHash) -> Result<(), RepoError> {
        validate_branch_name(name)?;
        let reference = tag_ref(name);
        if self.store.read_ref(&reference)?.is_some() {
            return Err(RepoError::TagExists(name.into()));
        }
        ensure_no_name_clash(&self.store, &reference)?;
        dag::load_commit(&self.store, &commit)?;
        if !self.store.compare_and_swap_ref(&reference, None, commit)? {
            return Err(RepoError::TagExists(name.into()));
        }
        Ok(())
    }

    /// The commit a tag points at, or `None` if there is no such tag.
    pub fn resolve_tag(&self, name: &str) -> Result<Option<ContentHash>, RepoError> {
        validate_branch_name(name)?;
        Ok(self.store.read_ref(&tag_ref(name))?)
    }

    /// All tag names, sorted.
    pub fn list_tags(&self) -> Result<Vec<String>, RepoError> {
        Ok(self
            .store
            .list_refs()?
            .into_iter()
            .filter_map(|name| name.strip_prefix(TAG_PREFIX).map(String::from))
            .collect())
    }

    /// Deletes a tag. Deleting is the only way to change a tag's target.
    pub fn delete_tag(&mut self, name: &str) -> Result<(), RepoError> {
        validate_branch_name(name)?;
        if self.store.delete_ref(&tag_ref(name))? {
            Ok(())
        } else {
            Err(RepoError::TagNotFound(name.into()))
        }
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

    /// Moves the current branch and the working copy to `commit`, discarding
    /// staged changes, pending conflicts and a pending merge. Detached, it moves
    /// only the working copy.
    pub fn reset_hard(&mut self, commit: ContentHash) -> Result<(), RepoError> {
        dag::load_commit(&self.store, &commit)?;
        self.move_to(commit)?;
        self.conflicts.clear();
        Ok(())
    }

    /// Stages the inverse of `commit`'s change on top of the head, as a
    /// three-way merge. The result is staged for review, like a merge: commit it
    /// once any conflicts are resolved.
    pub fn revert(&mut self, commit: ContentHash) -> Result<MergeResult, RepoError> {
        let (parent_tree, change) = self.change_of(&commit)?;
        // Undo the change: the base is the commit's own tree, theirs the parent's.
        self.apply_change(change, parent_tree)
    }

    /// Stages `commit`'s change on top of the head, as a three-way merge. The
    /// result is staged for review, like a merge: commit it once any conflicts
    /// are resolved.
    pub fn cherry_pick(&mut self, commit: ContentHash) -> Result<MergeResult, RepoError> {
        let (parent_tree, change) = self.change_of(&commit)?;
        self.apply_change(parent_tree, change)
    }

    /// The tree of `commit`'s first parent (empty for a root commit), and the
    /// tree of `commit` itself.
    fn change_of(&mut self, commit: &ContentHash) -> Result<(ContentHash, ContentHash), RepoError> {
        let loaded = dag::load_commit(&self.store, commit)?;
        let parent_tree = match loaded.parents.first() {
            Some(parent) => dag::commit_tree(&self.store, parent)?,
            None => store_object(&mut self.store, &Object::Tree(Tree::default()))?,
        };
        Ok((parent_tree, loaded.tree_hash))
    }

    /// Three-way merges `theirs` into the working copy, with `base` as the common
    /// ancestor, and stages the result.
    fn apply_change(
        &mut self,
        base: ContentHash,
        theirs: ContentHash,
    ) -> Result<MergeResult, RepoError> {
        self.ensure_clean()?;
        let ours = match self.head {
            Some(head) => dag::commit_tree(&self.store, &head)?,
            None => store_object(&mut self.store, &Object::Tree(Tree::default()))?,
        };
        let result = merge::merge_trees(&mut self.store, base, ours, theirs)?;
        self.staged = load_tree(&self.store, &result.merged_tree)?.entries;
        self.conflicts = result.conflicts.clone();
        Ok(result)
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
        let result = self.merge_branch(branch)?;
        if let Some(hooks) = self.hooks.as_mut() {
            hooks.post_merge(branch, &result);
        }
        Ok(result)
    }

    fn merge_branch(&mut self, branch: &str) -> Result<MergeResult, RepoError> {
        self.ensure_clean()?;
        let theirs = self
            .resolve_branch(branch)?
            .ok_or_else(|| RepoError::BranchNotFound(branch.into()))?;
        self.ensure_trusted_incoming(&theirs)?;
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
            Resolution::Custom(node) => {
                node.check_depth()?;
                Some(store_object(&mut self.store, &Object::Entity(node))?)
            }
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
        let staged = commit_entries(&self.store, &target)?;
        if let Some(name) = self.branch.clone() {
            self.advance_branch(&name, self.head, target)?;
        }
        self.staged = staged;
        self.head = Some(target);
        self.merge_parents.clear();
        Ok(())
    }

    /// Moves `name` from `expected` to `new`, refusing if another writer has
    /// moved it since this repository last read it.
    fn advance_branch(
        &mut self,
        name: &str,
        expected: Option<ContentHash>,
        new: ContentHash,
    ) -> Result<(), RepoError> {
        if self.store.compare_and_swap_ref(name, expected, new)? {
            Ok(())
        } else {
            Err(RepoError::RefMoved(name.into()))
        }
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

impl<S: CasStore + RefStore + PruneStore, C> Repository<S, C> {
    /// Removes stored objects that nothing can reach.
    ///
    /// The roots are every branch, the checked-out commit, the working copy and
    /// any pending merge, so uncommitted work is kept. If a reachable object is
    /// missing, nothing is removed and the error says which one.
    ///
    /// A writer in another process may store an object just before it updates a
    /// ref. Run `gc` only when no other writer is active on this store.
    pub fn gc(&mut self) -> Result<GcReport, RepoError> {
        let mut roots = Vec::new();
        for name in self.store.list_refs()? {
            roots.extend(self.store.read_ref(&name)?);
        }
        roots.extend(self.head);
        roots.extend(self.merge_parents.iter().copied());
        roots.extend(self.staged.values().copied());

        let live = maintenance::reachable(&self.store, roots)?;
        let mut removed = 0;
        for hash in self.store.hashes()? {
            if !live.contains(&hash) && self.store.delete(&hash)? {
                removed += 1;
            }
        }
        Ok(GcReport {
            kept: live.len(),
            removed,
        })
    }
}

/// Checks that an entity's stored form can be loaded again under its own id.
fn check_storable<E: Entity>(node: &UirNode, id: &EntityId) -> Result<(), RepoError> {
    node.check_depth()?;
    if E::from_uir(node)?.id() != *id {
        return Err(RepoError::IdMismatch(*id));
    }
    Ok(())
}

/// Refuses a new branch name that would collide with an existing one on a
/// case-insensitive file system (`Main` vs `main`), or that would need an
/// existing branch to be a directory (`a` vs `a/b`).
fn ensure_no_name_clash<S: RefStore>(store: &S, name: &str) -> Result<(), RepoError> {
    let lower = name.to_ascii_lowercase();
    let nested_under = alloc::format!("{lower}/");
    for existing in store.list_refs()? {
        let folded = existing.to_ascii_lowercase();
        let clashes = folded == lower
            || folded.starts_with(&nested_under)
            || lower.starts_with(&alloc::format!("{folded}/"));
        if clashes {
            return Err(RepoError::BranchNameClash {
                name: name.into(),
                existing,
            });
        }
    }
    Ok(())
}

fn commit_entries<S: CasStore>(
    store: &S,
    commit: &ContentHash,
) -> Result<BTreeMap<EntityId, ContentHash>, RepoError> {
    Ok(load_tree(store, &dag::commit_tree(store, commit)?)?.entries)
}
