//! A small command-line history for cells, kept in a directory on disk.
//!
//! ```text
//! cargo run --example file_store_cli -- <store-dir> commit <message> <address>=<text>...
//! cargo run --example file_store_cli -- <store-dir> log
//! cargo run --example file_store_cli -- <store-dir> show
//! ```
//!
//! Each `commit` stages the given cells and commits them to `main`. The store
//! directory holds everything, so the history survives between runs.

use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::process::ExitCode;

use tpt_version::error::ObjectError;

use tpt_version::repo::SystemClock;
use tpt_version::store::FileSystemStore;
use tpt_version::{ActorId, Entity, EntityId, Repository, Scalar, UirNode};

/// A cell keyed by its address, such as `A1`.
struct Cell {
    address: String,
    text: String,
}

fn id_for(address: &str) -> EntityId {
    let mut bytes = [0u8; 16];
    bytes[..address.len()].copy_from_slice(address.as_bytes());
    EntityId::from_bytes(bytes)
}

impl Entity for Cell {
    fn id(&self) -> EntityId {
        id_for(&self.address)
    }

    fn to_uir(&self) -> UirNode {
        UirNode::Map(BTreeMap::from([
            ("address".into(), UirNode::from(self.address.as_str())),
            ("text".into(), UirNode::from(self.text.as_str())),
        ]))
    }

    fn from_uir(node: &UirNode) -> Result<Self, ObjectError> {
        let UirNode::Map(fields) = node else {
            return Err(ObjectError::InvalidValue("cell"));
        };
        let text_of = |name| match fields.get(name) {
            Some(UirNode::Scalar(Scalar::Text(text))) => Ok(text.clone()),
            _ => Err(ObjectError::InvalidValue("cell field")),
        };
        Ok(Self {
            address: text_of("address")?,
            text: text_of("text")?,
        })
    }
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: file_store_cli <store-dir> (commit <message> <address>=<text>... | log | show)"
    );
    ExitCode::FAILURE
}

fn run(args: &[String]) -> Result<ExitCode, Box<dyn Error>> {
    let [dir, command, rest @ ..] = args else {
        return Ok(usage());
    };
    let store = FileSystemStore::open(dir)?;
    let mut repo = Repository::open(store, SystemClock, "main")?;
    let author = ActorId::from_bytes([0xC1; 16]);

    match (command.as_str(), rest) {
        ("commit", [message, pairs @ ..]) if !pairs.is_empty() => {
            for pair in pairs {
                let (address, text) = pair
                    .split_once('=')
                    .ok_or_else(|| format!("expected <address>=<text>, got {pair:?}"))?;
                repo.put_entity(&Cell {
                    address: address.into(),
                    text: text.into(),
                })?;
            }
            let hash = repo.commit(message, &author)?;
            println!("committed {hash}");
        }
        ("log", []) => match repo.head() {
            None => println!("no commits yet"),
            Some(_) => {
                for commit in repo.log("main")? {
                    println!("{}", commit.message);
                }
            }
        },
        ("show", []) => match repo.head() {
            None => println!("no commits yet"),
            Some(_) => {
                // The repository is already staged at the head, so each id loads.
                for id in repo.head_tree()?.entries.keys() {
                    if let Some(cell) = repo.entity::<Cell>(id)? {
                        println!("{} = {:?}", cell.address, cell.text);
                    }
                }
            }
        },
        _ => return Ok(usage()),
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
