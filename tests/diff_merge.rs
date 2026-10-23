mod common;

use std::collections::BTreeMap;

use common::{Vertex, author, id, new_repo, put, vertex};
use tpt_version::diff::{diff_commits, diff_entities};
use tpt_version::merge::merge_trees;
use tpt_version::object::Object;
use tpt_version::store::CasStore;
use tpt_version::{
    ContentHash, DiffEntry, Entity, EntityId, MemoryStore, RepoError, Resolution, Tree, UirNode,
};

/// Builds a tree from entity nodes and returns its hash.
fn tree_of(store: &mut MemoryStore, nodes: &[(EntityId, UirNode)]) -> ContentHash {
    let entries = nodes
        .iter()
        .map(|(id, node)| (*id, put(store, &Object::Entity(node.clone()))))
        .collect();
    put(store, &Object::Tree(Tree { entries }))
}

fn vertex_tree(store: &mut MemoryStore, vertices: &[Vertex]) -> ContentHash {
    let nodes: Vec<_> = vertices.iter().map(|v| (v.id(), v.to_uir())).collect();
    tree_of(store, &nodes)
}

/// Decodes one entity from a tree. `None` if absent or a conflict.
fn vertex_in(store: &MemoryStore, tree: &ContentHash, n: u8) -> Option<Vertex> {
    let Object::Tree(tree) = Object::decode(&store.get(tree).unwrap()).unwrap() else {
        panic!("not a tree");
    };
    let hash = tree.entries.get(&id(n))?;
    match Object::decode(&store.get(hash).unwrap()).unwrap() {
        Object::Entity(node) => Some(Vertex::from_uir(&node).unwrap()),
        _ => None,
    }
}

fn entry_count(store: &MemoryStore, tree: &ContentHash) -> usize {
    let Object::Tree(tree) = Object::decode(&store.get(tree).unwrap()).unwrap() else {
        panic!("not a tree");
    };
    tree.entries.len()
}

#[test]
fn moved_vertex_is_modified_not_removed_and_added() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0, 0.0, 0.0])).unwrap();
    let before = repo.commit("origin", &author()).unwrap();
    repo.put_entity(&vertex(1, [1.0, 1.0, 1.0])).unwrap();
    let after = repo.commit("moved", &author()).unwrap();

    let entries = diff_commits(repo.store(), &before, &after).unwrap();
    assert_eq!(entries.len(), 1);
    let DiffEntry::Modified {
        id: changed, diff, ..
    } = &entries[0]
    else {
        panic!("expected Modified, got {:?}", entries[0]);
    };
    assert_eq!(*changed, id(1));
    let mut paths: Vec<String> = diff
        .field_changes
        .iter()
        .map(|change| change.field_path.join("."))
        .collect();
    paths.sort();
    assert_eq!(paths, ["position.x", "position.y", "position.z"]);
}

#[test]
fn added_and_removed_entities_are_reported() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let before = repo.commit("one", &author()).unwrap();
    repo.remove_entity(&id(1)).unwrap();
    repo.put_entity(&vertex(2, [0.0; 3])).unwrap();
    let after = repo.commit("swap", &author()).unwrap();

    let entries = diff_commits(repo.store(), &before, &after).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(
        entries
            .iter()
            .any(|e| matches!(e, DiffEntry::Removed { id: r, .. } if *r == id(1)))
    );
    assert!(
        entries
            .iter()
            .any(|e| matches!(e, DiffEntry::Added { id: a, .. } if *a == id(2)))
    );
}

#[test]
fn identical_entities_produce_no_diff() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [2.0; 3])).unwrap();
    let first = repo.commit("a", &author()).unwrap();
    let second = repo.commit("b", &author()).unwrap();
    assert!(
        diff_commits(repo.store(), &first, &second)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn field_diff_reports_added_and_removed_keys() {
    let old = UirNode::Map(BTreeMap::from([
        ("kept".into(), 1_i64.into()),
        ("gone".into(), 2_i64.into()),
    ]));
    let new = UirNode::Map(BTreeMap::from([
        ("kept".into(), 1_i64.into()),
        ("fresh".into(), 3_i64.into()),
    ]));
    let diff = diff_entities(&old, &new);
    assert_eq!(diff.field_changes.len(), 2);
    let gone = diff
        .field_changes
        .iter()
        .find(|c| c.field_path == ["gone"])
        .unwrap();
    assert_eq!(
        (gone.old_value.is_some(), gone.new_value.is_none()),
        (true, true)
    );
    let fresh = diff
        .field_changes
        .iter()
        .find(|c| c.field_path == ["fresh"])
        .unwrap();
    assert_eq!(
        (fresh.old_value.is_none(), fresh.new_value.is_some()),
        (true, true)
    );
}

#[test]
fn change_on_one_side_is_taken_without_conflict() {
    let mut store = MemoryStore::default();
    let base = vertex_tree(&mut store, &[vertex(1, [0.0; 3])]);
    let ours = base;
    let theirs = vertex_tree(&mut store, &[vertex(1, [5.0; 3])]);
    let result = merge_trees(&mut store, base, ours, theirs).unwrap();
    assert!(result.conflicts.is_empty());
    assert_eq!(result.merged_tree, theirs);
}

#[test]
fn non_overlapping_field_edits_auto_merge_in_either_order() {
    let mut store = MemoryStore::default();
    let base = vertex_tree(&mut store, &[vertex(1, [0.0; 3])]);
    let x_moved = vertex_tree(&mut store, &[vertex(1, [1.0, 0.0, 0.0])]);
    let z_moved = vertex_tree(&mut store, &[vertex(1, [0.0, 0.0, 9.0])]);

    let forward = merge_trees(&mut store, base, x_moved, z_moved).unwrap();
    let backward = merge_trees(&mut store, base, z_moved, x_moved).unwrap();

    assert!(forward.conflicts.is_empty() && backward.conflicts.is_empty());
    assert_eq!(
        forward.merged_tree, backward.merged_tree,
        "merge should commute"
    );
    let merged = vertex_in(&store, &forward.merged_tree, 1).unwrap();
    assert_eq!(merged.position, [1.0, 0.0, 9.0]);
}

#[test]
fn identical_changes_collapse_without_conflict() {
    let mut store = MemoryStore::default();
    let base = vertex_tree(&mut store, &[vertex(1, [0.0; 3])]);
    let both = vertex_tree(&mut store, &[vertex(1, [3.0; 3])]);
    let result = merge_trees(&mut store, base, both, both).unwrap();
    assert!(result.conflicts.is_empty());
    assert_eq!(result.merged_tree, both);
}

#[test]
fn overlapping_field_edit_is_reported_with_all_three_values() {
    let mut store = MemoryStore::default();
    let base = vertex_tree(&mut store, &[vertex(1, [0.0; 3])]);
    let ours = vertex_tree(&mut store, &[vertex(1, [1.0, 0.0, 0.0])]);
    let theirs = vertex_tree(&mut store, &[vertex(1, [2.0, 0.0, 0.0])]);

    let result = merge_trees(&mut store, base, ours, theirs).unwrap();
    assert_eq!(result.conflicts.len(), 1);
    let conflict = &result.conflicts[0];
    assert_eq!(conflict.entity, id(1));
    assert_eq!(conflict.fields.len(), 1);
    let field = &conflict.fields[0];
    assert_eq!(field.path, ["position", "x"]);
    assert_eq!(field.base, Some(UirNode::from(0.0)));
    assert_eq!(field.ours, Some(UirNode::from(1.0)));
    assert_eq!(field.theirs, Some(UirNode::from(2.0)));

    // The merged tree points at the conflict object, not at a guessed entity.
    assert!(vertex_in(&store, &result.merged_tree, 1).is_none());
}

#[test]
fn delete_versus_modify_is_a_whole_entity_conflict() {
    let mut store = MemoryStore::default();
    let base = vertex_tree(&mut store, &[vertex(1, [0.0; 3]), vertex(2, [0.0; 3])]);
    let ours = vertex_tree(&mut store, &[vertex(2, [0.0; 3])]);
    let theirs = vertex_tree(&mut store, &[vertex(1, [4.0; 3]), vertex(2, [0.0; 3])]);

    let result = merge_trees(&mut store, base, ours, theirs).unwrap();
    assert_eq!(result.conflicts.len(), 1);
    let conflict = &result.conflicts[0];
    assert_eq!(conflict.entity, id(1));
    assert!(conflict.ours.is_none() && conflict.theirs.is_some());
    assert!(conflict.fields[0].path.is_empty());
    assert!(conflict.fields[0].ours.is_none());
}

#[test]
fn deletion_against_unchanged_entity_applies_cleanly() {
    let mut store = MemoryStore::default();
    let base = vertex_tree(&mut store, &[vertex(1, [0.0; 3]), vertex(2, [0.0; 3])]);
    let ours = vertex_tree(&mut store, &[vertex(2, [0.0; 3])]);
    let result = merge_trees(&mut store, base, ours, base).unwrap();
    assert!(result.conflicts.is_empty());
    assert_eq!(entry_count(&store, &result.merged_tree), 1);
}

#[test]
fn concurrent_additions_with_disjoint_fields_merge() {
    let mut store = MemoryStore::default();
    let base = tree_of(&mut store, &[]);
    let ours = tree_of(
        &mut store,
        &[(
            id(5),
            UirNode::Map(BTreeMap::from([("a".into(), 1_i64.into())])),
        )],
    );
    let theirs = tree_of(
        &mut store,
        &[(
            id(5),
            UirNode::Map(BTreeMap::from([("b".into(), 2_i64.into())])),
        )],
    );

    let result = merge_trees(&mut store, base, ours, theirs).unwrap();
    assert!(result.conflicts.is_empty());
    let Object::Tree(tree) = Object::decode(&store.get(&result.merged_tree).unwrap()).unwrap()
    else {
        panic!("not a tree");
    };
    let Object::Entity(UirNode::Map(fields)) =
        Object::decode(&store.get(&tree.entries[&id(5)]).unwrap()).unwrap()
    else {
        panic!("not a map entity");
    };
    assert_eq!(fields.len(), 2);
}

#[test]
fn commits_are_refused_until_conflicts_are_resolved() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let base = repo.commit("base", &author()).unwrap();
    repo.branch("rival", base).unwrap();

    repo.put_entity(&vertex(1, [1.0, 0.0, 0.0])).unwrap();
    repo.commit("main x=1", &author()).unwrap();
    repo.switch_branch("rival").unwrap();
    repo.put_entity(&vertex(1, [2.0, 0.0, 0.0])).unwrap();
    repo.commit("rival x=2", &author()).unwrap();
    repo.switch_branch("main").unwrap();

    let result = repo.merge("rival").unwrap();
    assert_eq!(result.conflicts.len(), 1);
    assert_eq!(repo.conflicts().len(), 1);
    assert_eq!(
        repo.commit("too early", &author()),
        Err(RepoError::UnresolvedConflicts(1))
    );
    assert_eq!(
        repo.put_entity(&vertex(1, [9.0; 3])),
        Err(RepoError::ConflictPending(id(1)))
    );
    assert_eq!(
        repo.resolve_conflict(&id(2), Resolution::Ours),
        Err(RepoError::NoConflict(id(2)))
    );

    repo.resolve_conflict(
        &id(1),
        Resolution::Custom(vertex(1, [7.0, 0.0, 0.0]).to_uir()),
    )
    .unwrap();
    let merge_head = repo.commit("resolved", &author()).unwrap();
    assert_eq!(
        repo.entity::<Vertex>(&id(1)).unwrap().unwrap().position,
        [7.0, 0.0, 0.0]
    );
    assert_eq!(repo.resolve_branch("main").unwrap(), Some(merge_head));
}

#[test]
fn resolving_with_theirs_takes_their_version() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let base = repo.commit("base", &author()).unwrap();
    repo.branch("rival", base).unwrap();
    repo.put_entity(&vertex(1, [1.0, 0.0, 0.0])).unwrap();
    repo.commit("ours", &author()).unwrap();
    repo.switch_branch("rival").unwrap();
    repo.put_entity(&vertex(1, [2.0, 0.0, 0.0])).unwrap();
    repo.commit("theirs", &author()).unwrap();
    repo.switch_branch("main").unwrap();

    repo.merge("rival").unwrap();
    repo.resolve_conflict(&id(1), Resolution::Theirs).unwrap();
    repo.commit("took theirs", &author()).unwrap();
    assert_eq!(
        repo.entity::<Vertex>(&id(1)).unwrap().unwrap().position,
        [2.0, 0.0, 0.0]
    );
}

#[test]
fn merge_commit_records_both_parents() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let base = repo.commit("base", &author()).unwrap();
    repo.branch("side", base).unwrap();
    repo.put_entity(&vertex(1, [1.0, 0.0, 0.0])).unwrap();
    let ours = repo.commit("ours", &author()).unwrap();
    repo.switch_branch("side").unwrap();
    repo.put_entity(&vertex(2, [0.0; 3])).unwrap();
    let theirs = repo.commit("theirs", &author()).unwrap();
    repo.switch_branch("main").unwrap();

    repo.merge("side").unwrap();
    let merge_commit = repo.commit("merge side", &author()).unwrap();

    let commit = tpt_version::dag::load_commit(repo.store(), &merge_commit).unwrap();
    assert_eq!(commit.parents, [ours, theirs]);
    assert!(repo.entity::<Vertex>(&id(2)).unwrap().is_some());
}

#[test]
fn merging_a_descendant_fast_forwards() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let base = repo.commit("base", &author()).unwrap();
    repo.branch("ahead", base).unwrap();
    repo.switch_branch("ahead").unwrap();
    repo.put_entity(&vertex(1, [6.0; 3])).unwrap();
    let ahead = repo.commit("ahead", &author()).unwrap();
    repo.switch_branch("main").unwrap();

    let result = repo.merge("ahead").unwrap();
    assert!(result.conflicts.is_empty());
    assert_eq!(repo.head(), Some(ahead));
    assert_eq!(repo.resolve_branch("main").unwrap(), Some(ahead));
}

#[test]
fn uncommitted_changes_block_a_merge() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let base = repo.commit("base", &author()).unwrap();
    repo.branch("other", base).unwrap();
    repo.put_entity(&vertex(1, [1.0; 3])).unwrap();
    assert_eq!(repo.merge("other"), Err(RepoError::DirtyWorkingCopy));
}
