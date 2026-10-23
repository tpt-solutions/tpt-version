//! JSON output for diffs and entity values, for viewers and scripts.
//!
//! Written by hand so the core stays free of serde. The output is standard
//! JSON. Values that JSON cannot express exactly are written as strings: bytes
//! as lowercase hex, and non-finite floats as `"NaN"`, `"Infinity"` or
//! `"-Infinity"`.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use crate::diff::{DiffEntry, FieldChange};
use crate::uir::{Scalar, UirNode};

/// Renders a list of entity changes as a JSON array.
pub fn diff_to_json(entries: &[DiffEntry]) -> String {
    let items: Vec<String> = entries.iter().map(entry_json).collect();
    alloc::format!("[{}]", items.join(","))
}

fn entry_json(entry: &DiffEntry) -> String {
    match entry {
        DiffEntry::Added { id, hash } => {
            alloc::format!(r#"{{"change":"added","id":"{id}","hash":"{hash}"}}"#)
        }
        DiffEntry::Removed { id, hash } => {
            alloc::format!(r#"{{"change":"removed","id":"{id}","hash":"{hash}"}}"#)
        }
        DiffEntry::Modified {
            id,
            old_hash,
            new_hash,
            diff,
        } => {
            let fields: Vec<String> = diff.field_changes.iter().map(field_json).collect();
            alloc::format!(
                r#"{{"change":"modified","id":"{id}","old_hash":"{old_hash}","new_hash":"{new_hash}","fields":[{}]}}"#,
                fields.join(",")
            )
        }
    }
}

fn field_json(change: &FieldChange) -> String {
    let path: Vec<String> = change.field_path.iter().map(|name| quoted(name)).collect();
    alloc::format!(
        r#"{{"path":[{}],"old":{},"new":{}}}"#,
        path.join(","),
        optional_node_json(change.old_value.as_ref()),
        optional_node_json(change.new_value.as_ref()),
    )
}

fn optional_node_json(node: Option<&UirNode>) -> String {
    match node {
        Some(node) => node_json(node),
        None => String::from("null"),
    }
}

/// Renders one value as JSON. Maps keep the key order of the stored value.
pub fn node_json(node: &UirNode) -> String {
    match node {
        UirNode::Scalar(scalar) => scalar_json(scalar),
        UirNode::List(items) => {
            let parts: Vec<String> = items.iter().map(node_json).collect();
            alloc::format!("[{}]", parts.join(","))
        }
        UirNode::Map(fields) => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(name, value)| alloc::format!("{}:{}", quoted(name), node_json(value)))
                .collect();
            alloc::format!("{{{}}}", parts.join(","))
        }
    }
}

fn scalar_json(scalar: &Scalar) -> String {
    match scalar {
        Scalar::Null => String::from("null"),
        Scalar::Bool(value) => String::from(if *value { "true" } else { "false" }),
        Scalar::Int(value) => alloc::format!("{value}"),
        Scalar::Float(value) if value.is_nan() => String::from("\"NaN\""),
        Scalar::Float(value) if value.is_infinite() && *value > 0.0 => String::from("\"Infinity\""),
        Scalar::Float(value) if value.is_infinite() => String::from("\"-Infinity\""),
        Scalar::Float(value) => alloc::format!("{value:?}"),
        Scalar::Text(text) => quoted(text),
        Scalar::Bytes(bytes) => {
            let mut hex = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                let _ = write!(hex, "{byte:02x}");
            }
            quoted(&hex)
        }
    }
}

/// A JSON string literal with the required escapes.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_strings_and_writes_non_finite_floats_as_text() {
        let node = UirNode::List(alloc::vec![
            UirNode::Scalar(Scalar::Text("a\"b\\c\n".into())),
            UirNode::Scalar(Scalar::Float(f64::NAN)),
            UirNode::Scalar(Scalar::Float(f64::NEG_INFINITY)),
            UirNode::Scalar(Scalar::Bytes(alloc::vec![0xAB, 0x01])),
            UirNode::Scalar(Scalar::Null),
        ]);
        assert_eq!(
            node_json(&node),
            r#"["a\"b\\c\n","NaN","-Infinity","ab01",null]"#
        );
    }

    #[test]
    fn floats_keep_a_decimal_point() {
        assert_eq!(node_json(&UirNode::Scalar(Scalar::Float(2.0))), "2.0");
    }
}
