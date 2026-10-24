//! `tpt`: version a key-value document kept in a directory.
//!
//! ```text
//! tpt [--dir DIR] commit <message> <key>=<value>...   record entries on the current branch
//! tpt [--dir DIR] get <key>                           print one value at the head
//! tpt [--dir DIR] list                                print every entry at the head
//! tpt [--dir DIR] log                                 print the history of the current branch
//! tpt [--dir DIR] branch <name>                       create a branch at the head
//! tpt [--dir DIR] branches                            list branches
//! tpt [--dir DIR] switch <name>                       check out a branch
//! tpt [--dir DIR] merge <branch> <message>            merge; conflicts stay pending
//! tpt [--dir DIR] merge <branch> <ours|theirs> <message>
//! tpt [--dir DIR] revert <rev>                         stage the inverse of a commit
//! tpt [--dir DIR] cherry-pick <rev>                     stage a commit's change on the head
//! tpt [--dir DIR] conflicts                           list the pending conflicts
//! tpt [--dir DIR] resolve <key> <ours|theirs>         settle one pending conflict
//! tpt [--dir DIR] finish-merge <message>              commit a merge once none remain
//! tpt [--dir DIR] tag <name>                          tag the head
//! tpt [--dir DIR] tags                                list tags
//! tpt [--dir DIR] fsck                                check the store
//! tpt [--dir DIR] gc                                  remove unreachable objects
//! ```
//!
//! The store defaults to `.tpt` in the current directory. Each command is one
//! step. A merge that leaves conflicts is saved in the store, so `conflicts`
//! and `resolve` can settle it in later runs.

use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use tpt_version::UirNode;
use tpt_version::diff::{DiffEntry, diff_commits};
use tpt_version::export::node_json;
use tpt_version::maintenance::fsck;
use tpt_version::object::Object;
use tpt_version::prelude::*;
use tpt_version::repo::SystemClock;
use tpt_version::store::FileSystemStore;

type Failure = Box<dyn Error>;

/// One key-value pair. The id is derived from the key, so an entry keeps its
/// identity when its value changes.
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

fn usage() -> ExitCode {
    eprintln!(
        "usage: tpt [--dir DIR] <commit|get|list|log|branch|branches|switch|merge|revert|cherry-pick|conflicts|resolve|finish-merge|tag|tags|fsck|gc> ..."
    );
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("tpt: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(mut args: Vec<String>) -> Result<ExitCode, Failure> {
    let dir = if args.first().map(String::as_str) == Some("--dir") {
        if args.len() < 2 {
            return Ok(usage());
        }
        args.remove(0);
        PathBuf::from(args.remove(0))
    } else {
        PathBuf::from(".tpt")
    };
    if args.is_empty() {
        return Ok(usage());
    }
    let command = args.remove(0);

    if command == "init" {
        if dir.join("HEAD").exists() {
            return Err(format!("{} is already a tpt store", dir.display()).into());
        }
        FileSystemStore::open(&dir)?;
        fs::write(dir.join("HEAD"), tpt_version::repo::DEFAULT_BRANCH)?;
        println!("initialised {}", dir.display());
        return Ok(ExitCode::SUCCESS);
    }

    let store = FileSystemStore::open(&dir)?;
    let branch = fs::read_to_string(dir.join("HEAD"))
        .map(|name| name.trim().to_string())
        .unwrap_or_else(|_| tpt_version::repo::DEFAULT_BRANCH.into());
    let mut repo = Repository::open(store, SystemClock, &branch)?;
    let author = ActorId::from_bytes([0x7; 16]);

    match (command.as_str(), args.as_slice()) {
        ("commit", [message, pairs @ ..]) if !pairs.is_empty() => {
            for pair in pairs {
                let (key, value) = pair
                    .split_once('=')
                    .ok_or_else(|| format!("expected <key>=<value>, got {pair:?}"))?;
                repo.put_entity(&entry(key, value))?;
            }
            let hash = repo.commit(message, &author)?;
            println!("{hash}");
        }
        ("get", [key]) => match repo.entity::<Entry>(&entry(key, "").id)? {
            Some(found) => println!("{}", found.value),
            None => {
                eprintln!("tpt: no entry for {key:?}");
                return Ok(ExitCode::FAILURE);
            }
        },
        ("list", []) => {
            let mut entries: Vec<Entry> = head_entries(&repo)?.into_values().collect();
            entries.sort_by(|a, b| a.key.cmp(&b.key));
            for found in entries {
                println!("{}={}", found.key, found.value);
            }
        }
        ("log", []) => {
            for commit in repo.log(&branch)? {
                println!("{}", commit.message);
            }
        }
        ("branch", [name]) => {
            let head = head_commit(&repo)?;
            repo.branch(name, head)?;
        }
        ("branches", []) => {
            for name in repo.list_branches()? {
                let marker = if name == branch { "*" } else { " " };
                println!("{marker} {name}");
            }
        }
        ("switch", [name]) => {
            repo.switch_branch(name)?;
            fs::write(dir.join("HEAD"), name)?;
        }
        ("merge", [source, message]) => merge_into_head(&mut repo, source, None, message, &author)?,
        ("merge", [source, prefer, message]) => {
            merge_into_head(&mut repo, source, Some(prefer), message, &author)?;
        }
        ("conflicts", []) => {
            let conflicts = repo.conflicts().to_vec();
            if conflicts.is_empty() {
                println!("no conflicts");
            }
            for conflict in &conflicts {
                let ours = stored_entry(&repo, conflict.ours)?;
                let theirs = stored_entry(&repo, conflict.theirs)?;
                let base = stored_entry(&repo, conflict.base)?;
                let key = ours
                    .as_ref()
                    .or(theirs.as_ref())
                    .or(base.as_ref())
                    .map_or("?", |found| found.key.as_str());
                let side = |found: &Option<Entry>| match found {
                    Some(found) => format!("{:?}", found.value),
                    None => "(deleted)".into(),
                };
                println!("{key}: ours {} | theirs {}", side(&ours), side(&theirs));
            }
        }
        ("resolve", [key, prefer]) => {
            let resolution = parse_resolution(prefer)?;
            let id = entry(key, "").id;
            if !repo
                .conflicts()
                .iter()
                .any(|conflict| conflict.entity == id)
            {
                return Err(
                    format!("{key:?} has no pending conflict; run conflicts to list them").into(),
                );
            }
            repo.resolve_conflict(&id, resolution)?;
            println!("resolved {key:?} with {prefer}");
        }
        ("revert", [rev]) => {
            let commit = resolve(&repo, rev)?;
            repo.revert(commit)?;
            report_staged(&repo)?;
        }
        ("cherry-pick", [rev]) => {
            let commit = resolve(&repo, rev)?;
            repo.cherry_pick(commit)?;
            report_staged(&repo)?;
        }
        ("finish-merge", [message]) => {
            if !repo.status()?.staged {
                return Err(
                    "nothing is staged; start one with merge, revert or cherry-pick".into(),
                );
            }
            let hash = repo.commit(message, &author)?;
            println!("{hash}");
        }
        ("tag", [name]) => {
            let head = head_commit(&repo)?;
            repo.create_tag(name, head)?;
        }
        ("tags", []) => {
            for name in repo.list_tags()? {
                println!("{name}");
            }
        }
        ("fsck", []) => {
            let report = fsck(repo.store())?;
            println!("checked {} object(s)", report.objects_checked);
            for problem in &report.problems {
                println!("problem: {problem:?}");
            }
            if !report.is_clean() {
                return Ok(ExitCode::FAILURE);
            }
        }
        ("diff", [from, to]) => {
            let (before, after) = (resolve(&repo, from)?, resolve(&repo, to)?);
            for change in diff_commits(repo.store(), &before, &after)? {
                match change {
                    DiffEntry::Added { id, .. } => {
                        let found: Entry = required(repo.entity_at(&after, &id)?)?;
                        println!("added {}={}", found.key, found.value);
                    }
                    DiffEntry::Removed { id, .. } => {
                        let found: Entry = required(repo.entity_at(&before, &id)?)?;
                        println!("removed {}", found.key);
                    }
                    DiffEntry::Modified { id, diff, .. } => {
                        let found: Entry = required(repo.entity_at(&after, &id)?)?;
                        for field in &diff.field_changes {
                            let show = |value: &Option<UirNode>| match value {
                                Some(node) => node_json(node),
                                None => "null".into(),
                            };
                            println!(
                                "changed {} {}: {} -> {}",
                                found.key,
                                field.field_path.join("."),
                                show(&field.old_value),
                                show(&field.new_value)
                            );
                        }
                    }
                }
            }
        }
        ("gc", []) => {
            let report = repo.gc()?;
            println!("kept {}, removed {}", report.kept, report.removed);
        }
        _ => return Ok(usage()),
    }
    Ok(ExitCode::SUCCESS)
}

/// Merges `source` into the current branch. With `prefer`, every conflict is
/// settled that way and the merge is committed. Without it, conflicts are left
/// pending for `conflicts` and `resolve`, and `finish-merge` commits them.
fn merge_into_head(
    repo: &mut Repository<FileSystemStore, SystemClock>,
    source: &str,
    prefer: Option<&str>,
    message: &str,
    author: &ActorId,
) -> Result<(), Failure> {
    let result = repo.merge(source)?;
    if let Some(prefer) = prefer {
        let resolution = parse_resolution(prefer)?;
        for conflict in &result.conflicts {
            repo.resolve_conflict(&conflict.entity, resolution.clone())?;
        }
        if !result.conflicts.is_empty() {
            println!(
                "resolved {} conflict(s) with {prefer}",
                result.conflicts.len()
            );
        }
    }
    let status = repo.status()?;
    if !status.merging {
        // Fast-forwarded, or already up to date: there is nothing to commit.
        println!(
            "{}",
            repo.head()
                .map_or("no commits".into(), |head| head.to_string())
        );
    } else if !status.conflicts.is_empty() {
        println!(
            "merge left {} conflict(s) pending; run conflicts, then resolve <key> ours|theirs for each, then finish-merge <message>",
            status.conflicts.len()
        );
    } else {
        let hash = repo.commit(message, author)?;
        println!("{hash}");
    }
    Ok(())
}

/// Says what a revert or cherry-pick left staged, and the next step.
fn report_staged(repo: &Repository<FileSystemStore, SystemClock>) -> Result<(), Failure> {
    let conflicts = repo.status()?.conflicts.len();
    if conflicts == 0 {
        println!("staged; run finish-merge <message> to commit it");
    } else {
        println!(
            "staged with {conflicts} conflict(s); run conflicts, then resolve <key> ours|theirs for each, then finish-merge <message>"
        );
    }
    Ok(())
}

fn parse_resolution(prefer: &str) -> Result<Resolution, Failure> {
    match prefer {
        "ours" => Ok(Resolution::Ours),
        "theirs" => Ok(Resolution::Theirs),
        other => Err(format!("prefer must be ours or theirs, not {other:?}").into()),
    }
}

/// The entry a stored entity object holds, or `None` for a deleted side.
fn stored_entry(
    repo: &Repository<FileSystemStore, SystemClock>,
    hash: Option<ContentHash>,
) -> Result<Option<Entry>, Failure> {
    let Some(hash) = hash else {
        return Ok(None);
    };
    match Object::decode(&repo.store().get(&hash)?)? {
        Object::Entity(node) => Ok(Some(Entry::from_uir(&node)?)),
        _ => Err("a conflict points at an object that is not an entity".into()),
    }
}

/// Turns a branch name, a tag name or a full commit hash into a commit.
fn resolve(
    repo: &Repository<FileSystemStore, SystemClock>,
    name: &str,
) -> Result<ContentHash, Failure> {
    if let Some(commit) = repo.resolve_branch(name)? {
        return Ok(commit);
    }
    if let Some(commit) = repo.resolve_tag(name)? {
        return Ok(commit);
    }
    name.parse()
        .map_err(|_| format!("{name:?} is not a branch, a tag or a commit").into())
}

fn required<T>(found: Option<T>) -> Result<T, Failure> {
    found.ok_or_else(|| "entry is missing from that commit".to_string().into())
}

fn head_commit(repo: &Repository<FileSystemStore, SystemClock>) -> Result<ContentHash, Failure> {
    repo.head()
        .ok_or_else(|| "nothing committed yet".to_string().into())
}

/// The entries in the head snapshot, keyed by entity id.
fn head_entries(
    repo: &Repository<FileSystemStore, SystemClock>,
) -> Result<BTreeMap<EntityId, Entry>, Failure> {
    let mut entries = BTreeMap::new();
    for id in repo.head_tree()?.entries.keys() {
        if let Some(found) = repo.entity::<Entry>(id)? {
            entries.insert(*id, found);
        }
    }
    Ok(entries)
}
