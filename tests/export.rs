//! JSON output of diffs.

mod common;

use common::{author, id, new_repo, vertex};
use tpt_version::diff::diff_commits;
use tpt_version::export::diff_to_json;

#[test]
fn modified_entity_lists_each_changed_field() {
    let mut repo = new_repo();
    repo.put_entity(&vertex(1, [0.0; 3])).unwrap();
    let before = repo.commit("before", &author()).unwrap();
    repo.put_entity(&vertex(1, [1.0, 0.0, 2.5])).unwrap();
    let after = repo.commit("after", &author()).unwrap();

    let json = diff_to_json(&diff_commits(repo.store(), &before, &after).unwrap());
    assert!(
        json.starts_with(r#"[{"change":"modified","id":""#),
        "{json}"
    );
    assert!(json.contains(&format!(r#""id":"{}""#, id(1))), "{json}");
    assert!(
        json.contains(r#"{"path":["position","x"],"old":0.0,"new":1.0}"#),
        "{json}"
    );
    assert!(
        json.contains(r#"{"path":["position","z"],"old":0.0,"new":2.5}"#),
        "{json}"
    );
    assert!(
        !json.contains(r#""path":["id"]"#),
        "unchanged fields are omitted: {json}"
    );
}

#[test]
fn an_empty_diff_is_an_empty_array() {
    assert_eq!(diff_to_json(&[]), "[]");
}

#[test]
fn html_escapes_entity_content() {
    use tpt_version::diff::{DiffEntry, EntityDiff, FieldChange};
    use tpt_version::export::diff_to_html;
    use tpt_version::{ContentHash, Scalar, UirNode};

    let modified = DiffEntry::Modified {
        id: id(9),
        old_hash: ContentHash::of(b"old"),
        new_hash: ContentHash::of(b"new"),
        diff: EntityDiff {
            field_changes: vec![FieldChange {
                field_path: vec!["<b>".into()],
                old_value: None,
                new_value: Some(UirNode::Scalar(Scalar::Text("</td><script>".into()))),
            }],
        },
    };
    let html = diff_to_html(&[modified]);
    assert!(html.starts_with("<table"), "{html}");
    assert!(html.contains("<td>&lt;b&gt;</td>"), "{html}");
    assert!(html.contains("&lt;/td&gt;&lt;script&gt;"), "{html}");
    assert!(!html.contains("<script>"), "{html}");
}
