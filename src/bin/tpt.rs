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
//! tpt [--dir DIR] merge <branch> <ours|theirs> <message>
//! tpt [--dir DIR] tag <name>                          tag the head
//! tpt [--dir DIR] tags                                list tags
//! tpt [--dir DIR] fsck                                check the store
//! tpt [--dir DIR] gc                                  remove unreachable objects
//! ```
//!
//! The store defaults to `.tpt` in the current directory. Each command is one
//! step, so the working copy never has to outlive the process.

use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use tpt_version::dag;
use tpt_version::maintenance::fsck;
use tpt_version::object::Object;
use tpt_version::prelude::*;
use tpt_version::repo::SystemClock;
use tpt_version::store::{CasStore, FileSystemStore};

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
        "usage: tpt [--dir DIR] <commit|get|list|log|branch|branches|switch|merge|tag|tags|fsck|gc> ..."
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
        ("merge", [source, prefer, message]) => {
            let resolution = match prefer.as_str() {
                "ours" => Resolution::Ours,
                "theirs" => Resolution::Theirs,
                other => return Err(format!("prefer must be ours or theirs, not {other:?}").into()),
            };
            let result = repo.merge(source)?;
            for conflict in &result.conflicts {
                repo.resolve_conflict(&conflict.entity, resolution.clone())?;
            }
            if !result.conflicts.is_empty() {
                println!(
                    "resolved {} conflict(s) with {prefer}",
                    result.conflicts.len()
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
        ("gc", []) => {
            let report = repo.gc()?;
            println!("kept {}, removed {}", report.kept, report.removed);
        }
        _ => return Ok(usage()),
    }
    Ok(ExitCode::SUCCESS)
}

fn head_commit(repo: &Repository<FileSystemStore, SystemClock>) -> Result<ContentHash, Failure> {
    repo.head()
        .ok_or_else(|| "nothing committed yet".to_string().into())
}

/// The entries in the head snapshot, keyed by entity id.
fn head_entries(
    repo: &Repository<FileSystemStore, SystemClock>,
) -> Result<BTreeMap<EntityId, Entry>, Failure> {
    let Some(head) = repo.head() else {
        return Ok(BTreeMap::new());
    };
    let tree_hash = dag::load_commit(repo.store(), &head)?.tree_hash;
    let Object::Tree(tree) = Object::decode(&repo.store().get(&tree_hash)?)? else {
        return Err("head commit does not point at a tree".into());
    };
    let mut entries = BTreeMap::new();
    for id in tree.entries.keys() {
        if let Some(found) = repo.entity::<Entry>(id)? {
            entries.insert(*id, found);
        }
    }
    Ok(entries)
}
