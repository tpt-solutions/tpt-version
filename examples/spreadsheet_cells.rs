//! Two people edit one spreadsheet. Edits to different cells merge without
//! help. Two edits to the same cell become a conflict that someone resolves.
//!
//! Run with `cargo run --example spreadsheet_cells`.

use std::collections::BTreeMap;

use tpt_version::error::ObjectError;
use tpt_version::{
    ActorId, Entity, EntityId, LogicalClock, MemoryStore, RepoError, Repository, Resolution,
    Scalar, UirNode,
};

/// One cell, keyed by its address such as `A1`. The id is derived from the
/// address, so a cell keeps its identity when its text changes.
struct Cell {
    address: String,
    text: String,
}

impl Cell {
    fn new(address: &str, text: &str) -> Self {
        Self {
            address: address.into(),
            text: text.into(),
        }
    }
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

const ADDRESSES: [&str; 3] = ["A1", "A2", "A3"];

fn show(repo: &Repository<MemoryStore, LogicalClock>, addresses: &[&str]) -> Result<(), RepoError> {
    for address in addresses {
        match repo.entity::<Cell>(&id_for(address))? {
            Some(cell) => println!("  {address} = {:?}", cell.text),
            None => println!("  {address} is empty"),
        }
    }
    Ok(())
}

fn main() -> Result<(), RepoError> {
    let alice = ActorId::from_bytes([1; 16]);
    let bob = ActorId::from_bytes([2; 16]);
    let carol = ActorId::from_bytes([3; 16]);
    let mut repo = Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main")?;

    repo.put_entity(&Cell::new("A1", "Units"))?;
    repo.put_entity(&Cell::new("A2", "Width"))?;
    let base = repo.commit("layout", &alice)?;
    repo.branch("alice", base)?;
    repo.branch("carol", base)?;

    // Bob changes A2 on main. Alice changes A1 and adds A3 on her branch.
    repo.put_entity(&Cell::new("A2", "Width (cm)"))?;
    repo.commit("bob: width in cm", &bob)?;
    repo.switch_branch("alice")?;
    repo.put_entity(&Cell::new("A1", "Units (mm)"))?;
    repo.put_entity(&Cell::new("A3", "Total"))?;
    repo.commit("alice: units and total", &alice)?;

    // Different cells, so the merge is clean.
    repo.switch_branch("main")?;
    let merged = repo.merge("alice")?;
    println!("merge of alice: {} conflict(s)", merged.conflicts.len());
    repo.commit("merge alice", &bob)?;
    show(&repo, &ADDRESSES)?;

    // Carol and Bob both edit A2 on top of the merge, so it is a real conflict.
    repo.switch_branch("carol")?;
    repo.put_entity(&Cell::new("A2", "Width (in)"))?;
    repo.commit("carol: width in inches", &carol)?;
    repo.switch_branch("main")?;
    repo.put_entity(&Cell::new("A2", "Width (m)"))?;
    repo.commit("bob: width in metres", &bob)?;

    let result = repo.merge("carol")?;
    for conflict in &result.conflicts {
        let address = ADDRESSES
            .into_iter()
            .find(|address| id_for(address) == conflict.entity)
            .unwrap_or("unknown");
        println!("conflict on {address}");
    }
    // Nothing is resolved silently. Commits are refused until each conflict is
    // handled, so the merge is settled here with a value neither side wrote.
    repo.resolve_conflict(
        &id_for("A2"),
        Resolution::Custom(Cell::new("A2", "Width (m, converted from in)").to_uir()),
    )?;
    repo.commit("merge carol, width in metres", &bob)?;
    show(&repo, &ADDRESSES)?;
    Ok(())
}
