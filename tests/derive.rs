//! `#[derive(Entity)]` round trips a struct through its stored form, and works
//! inside a repository like a hand-written entity.

use tpt_version::prelude::*;

#[derive(Entity, Debug, Clone, PartialEq)]
struct Part {
    #[entity(id)]
    id: EntityId,
    position: [f64; 3],
    label: String,
    tags: Vec<String>,
    note: Option<String>,
    count: i32,
    visible: bool,
}

fn part(n: u8, label: &str) -> Part {
    Part {
        id: EntityId::from_bytes([n; 16]),
        position: [1.0, 2.0, 3.0],
        label: label.into(),
        tags: vec!["cad".into(), "steel".into()],
        note: None,
        count: -4,
        visible: true,
    }
}

#[test]
fn derived_entity_round_trips_through_its_stored_form() {
    let original = part(1, "bracket");
    let node = original.to_uir();
    assert_eq!(Part::from_uir(&node).unwrap(), original);
    assert_eq!(original.id(), EntityId::from_bytes([1; 16]));
}

#[test]
fn optional_fields_round_trip_both_ways() {
    let mut with_note = part(2, "hinge");
    with_note.note = Some("check tolerance".into());
    assert_eq!(Part::from_uir(&with_note.to_uir()).unwrap(), with_note);
}

#[test]
fn derived_entity_works_in_a_repository() {
    let mut repo =
        Repository::open(MemoryStore::default(), LogicalClock::starting_at(1), "main").unwrap();
    repo.put_entity(&part(3, "plate")).unwrap();
    repo.commit("add plate", &ActorId::from_bytes([1; 16]))
        .unwrap();

    repo.put_entity(&Part {
        label: "plate, drilled".into(),
        ..part(3, "plate")
    })
    .unwrap();
    repo.commit("drill", &ActorId::from_bytes([1; 16])).unwrap();

    let loaded: Part = repo
        .entity(&EntityId::from_bytes([3; 16]))
        .unwrap()
        .unwrap();
    assert_eq!(loaded.label, "plate, drilled");
    assert_eq!(repo.log("main").unwrap().len(), 2);
}

#[test]
fn a_stored_form_missing_a_field_is_refused() {
    let mut node = part(4, "pin").to_uir();
    if let UirNode::Map(fields) = &mut node {
        fields.remove("count");
    }
    assert_eq!(
        Part::from_uir(&node),
        Err(ObjectError::InvalidValue("count"))
    );
}
