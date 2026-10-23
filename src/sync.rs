//! Moving branches between stores: `push` sends a local branch to a remote
//! store, and `fetch` brings a remote branch into a local one.
//!
//! A store is its own remote. Any [`CasStore`] + [`RefStore`] can be either
//! side, including a `MemoryStore`, a `FileSystemStore` or an `S3Store`. Both
//! operations copy only the objects the destination lacks, and they move the
//! destination's ref with compare-and-swap, so a concurrent writer is detected
//! rather than overwritten.

use crate::dag;
use crate::error::RepoError;
use crate::maintenance::reachable;
use crate::object::ContentHash;
use crate::store::{CasStore, RefStore};

/// What a transfer did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transfer {
    /// Objects copied into the destination.
    pub objects_copied: usize,
    /// The commit the destination's branch now points at.
    pub head: ContentHash,
    /// Whether the destination's ref moved. `false` when it was already there.
    pub updated: bool,
}

/// Sends `branch` from `local` to `remote`. Refused unless the remote's branch
/// is an ancestor of the local one, so no commits are lost on the remote.
pub fn push<L, R>(local: &L, remote: &mut R, branch: &str) -> Result<Transfer, RepoError>
where
    L: CasStore + RefStore,
    R: CasStore + RefStore,
{
    transfer(local, remote, branch)
}

/// Brings `branch` from `remote` into `local`, with the same fast-forward rule
/// as [`push`]. Fetching does not change the local working copy: open the
/// branch again to see the new head.
pub fn fetch<R, L>(remote: &R, local: &mut L, branch: &str) -> Result<Transfer, RepoError>
where
    R: CasStore + RefStore,
    L: CasStore + RefStore,
{
    transfer(remote, local, branch)
}

fn transfer<F, T>(from: &F, to: &mut T, branch: &str) -> Result<Transfer, RepoError>
where
    F: CasStore + RefStore,
    T: CasStore + RefStore,
{
    let head = from
        .read_ref(branch)?
        .ok_or_else(|| RepoError::BranchNotFound(branch.into()))?;

    let mut objects_copied = 0;
    for hash in reachable(from, [head])? {
        if !to.contains(&hash)? {
            to.put(hash, &from.get(&hash)?)?;
            objects_copied += 1;
        }
    }

    let current = to.read_ref(branch)?;
    if current == Some(head) {
        return Ok(Transfer {
            objects_copied,
            head,
            updated: false,
        });
    }
    if let Some(current) = current {
        // The destination must already contain `current` (it was just checked
        // against `reachable` from the source, or it is local history), so the
        // ancestry test reads from the destination.
        if !dag::is_ancestor(to, &current, &head)? {
            return Err(RepoError::NotFastForward(branch.into()));
        }
    }
    if !to.compare_and_swap_ref(branch, current, head)? {
        return Err(RepoError::RefMoved(branch.into()));
    }
    Ok(Transfer {
        objects_copied,
        head,
        updated: true,
    })
}
