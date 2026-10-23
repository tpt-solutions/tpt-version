//! Content-addressed objects and their canonical encodings.
//!
//! Every object is a type tag followed by its fields. Its [`ContentHash`] is
//! the SHA-256 of those bytes, so the tag is part of the hashed content and
//! two kinds of object can never share a hash.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::str::FromStr;

use tpt_crypto_hash::sha2::sha256;

use crate::codec::{Reader, Writer};
use crate::error::{ObjectError, RepoError};
use crate::sign::Signature;
use crate::uir::{self, UirNode};

const TAG_BLOB: u8 = 1;
const TAG_ENTITY: u8 = 2;
const TAG_TREE: u8 = 3;
const TAG_COMMIT: u8 = 4;
const TAG_CONFLICT: u8 = 5;

fn write_hex(f: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    for byte in bytes {
        write!(f, "{byte:02x}")?;
    }
    Ok(())
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn parse_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (slot, pair) in out.iter_mut().zip(text.as_bytes().chunks(2)) {
        *slot = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Some(out)
}

macro_rules! identifier {
    ($(#[$doc:meta])* $name:ident, $len:literal) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name([u8; $len]);

        impl $name {
            /// Wraps raw bytes.
            pub const fn from_bytes(bytes: [u8; $len]) -> Self {
                Self(bytes)
            }

            /// The raw bytes.
            pub const fn as_bytes(&self) -> &[u8; $len] {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write_hex(f, &self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({self})", stringify!($name))
            }
        }

        impl FromStr for $name {
            type Err = ObjectError;

            fn from_str(text: &str) -> Result<Self, ObjectError> {
                parse_hex(text)
                    .map($name)
                    .ok_or(ObjectError::InvalidValue("hex identifier"))
            }
        }
    };
}

identifier! {
    /// The SHA-256 address of an object's canonical bytes.
    ContentHash, 32
}

identifier! {
    /// A stable entity identity, assigned once and kept across edits.
    EntityId, 16
}

identifier! {
    /// The identity of a commit author.
    ActorId, 16
}

impl ContentHash {
    /// The address of `data`.
    pub fn of(data: &[u8]) -> Self {
        Self(sha256(data))
    }
}

/// A snapshot of entities: each entity id maps to the hash of its current version.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tree {
    pub entries: BTreeMap<EntityId, ContentHash>,
}

/// A snapshot of a tree, with its history. Forms the Merkle DAG.
///
/// The signature is part of the encoded commit, so it is part of the commit
/// hash. The same content signed by two keys therefore has two addresses. The
/// signature covers [`Commit::signing_bytes`], which excludes the signature
/// itself, so there is no circularity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub tree_hash: ContentHash,
    pub parents: Vec<ContentHash>,
    pub author: ActorId,
    pub timestamp: u64,
    pub message: String,
    pub signature: Option<Signature>,
}

impl Commit {
    /// The bytes a signature covers: this commit encoded without its signature.
    pub fn signing_bytes(&self) -> Vec<u8> {
        encode_commit(self, false)
    }
}

/// A three-way merge that could not be resolved automatically.
///
/// Stored in the CAS and referenced from the merged tree in place of the
/// entity. The `ours` and `theirs` hashes name the two entity versions, so a
/// resolution can pick either one.
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    pub entity: EntityId,
    pub base: Option<ContentHash>,
    pub ours: Option<ContentHash>,
    pub theirs: Option<ContentHash>,
    pub fields: Vec<FieldConflict>,
}

/// One field that changed differently on both sides. An empty `path` means
/// the whole entity, which happens for delete-versus-modify.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldConflict {
    pub path: Vec<String>,
    pub base: Option<UirNode>,
    pub ours: Option<UirNode>,
    pub theirs: Option<UirNode>,
}

/// Any object the store holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Object {
    Blob(Vec<u8>),
    Entity(UirNode),
    Tree(Tree),
    Commit(Commit),
    Conflict(Conflict),
}

impl Object {
    /// A short name for the kind, used in error messages.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Blob(_) => "blob",
            Self::Entity(_) => "entity",
            Self::Tree(_) => "tree",
            Self::Commit(_) => "commit",
            Self::Conflict(_) => "conflict",
        }
    }

    /// The canonical bytes, including the leading type tag.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Blob(data) => {
                let mut w = Writer::new(TAG_BLOB);
                w.bytes(data);
                w.finish()
            }
            Self::Entity(node) => {
                let mut w = Writer::new(TAG_ENTITY);
                node.encode(&mut w);
                w.finish()
            }
            Self::Tree(tree) => {
                let mut w = Writer::new(TAG_TREE);
                w.count(tree.entries.len());
                for (id, hash) in &tree.entries {
                    w.fixed(id.as_bytes());
                    w.fixed(hash.as_bytes());
                }
                w.finish()
            }
            Self::Commit(commit) => encode_commit(commit, true),
            Self::Conflict(conflict) => encode_conflict(conflict),
        }
    }

    /// Decodes canonical bytes produced by [`Object::encode`].
    pub fn decode(data: &[u8]) -> Result<Self, ObjectError> {
        let (tag, mut r) = Reader::open(data)?;
        let object = match tag {
            TAG_BLOB => Self::Blob(r.bytes()?),
            TAG_ENTITY => Self::Entity(UirNode::decode(&mut r)?),
            TAG_TREE => Self::Tree(decode_tree(&mut r)?),
            TAG_COMMIT => Self::Commit(decode_commit(&mut r)?),
            TAG_CONFLICT => Self::Conflict(decode_conflict(&mut r)?),
            other => return Err(ObjectError::UnknownTag(other)),
        };
        r.finish()?;
        Ok(object)
    }

    /// The address of this object: the hash of its canonical bytes.
    pub fn hash(&self) -> ContentHash {
        ContentHash::of(&self.encode())
    }
}

/// The error for an object of the wrong kind. Used by loaders.
pub(crate) fn wrong_kind(expected: &'static str, found: &Object) -> RepoError {
    RepoError::Object(ObjectError::WrongKind {
        expected,
        found: found.kind(),
    })
}

fn encode_commit(commit: &Commit, with_signature: bool) -> Vec<u8> {
    let mut w = Writer::new(TAG_COMMIT);
    w.fixed(commit.tree_hash.as_bytes());
    w.count(commit.parents.len());
    for parent in &commit.parents {
        w.fixed(parent.as_bytes());
    }
    w.fixed(commit.author.as_bytes());
    w.u64(commit.timestamp);
    w.str(&commit.message);
    match (&commit.signature, with_signature) {
        (Some(signature), true) => {
            w.u8(1);
            w.fixed(&signature.signer);
            w.fixed(&signature.bytes);
        }
        _ => w.u8(0),
    }
    w.finish()
}

fn decode_commit(r: &mut Reader<'_>) -> Result<Commit, ObjectError> {
    let tree_hash = r.hash()?;
    let parent_count = r.count()?;
    let mut parents = Vec::new();
    for _ in 0..parent_count {
        parents.push(r.hash()?);
    }
    let author = ActorId::from_bytes(r.fixed()?);
    let timestamp = r.u64()?;
    let message = r.str()?;
    let signature = match r.u8()? {
        0 => None,
        1 => Some(Signature {
            signer: r.fixed()?,
            bytes: r.fixed()?,
        }),
        _ => return Err(ObjectError::InvalidValue("signature flag")),
    };
    Ok(Commit {
        tree_hash,
        parents,
        author,
        timestamp,
        message,
        signature,
    })
}

fn decode_tree(r: &mut Reader<'_>) -> Result<Tree, ObjectError> {
    let count = r.count()?;
    let mut entries = BTreeMap::new();
    for _ in 0..count {
        let id = EntityId::from_bytes(r.fixed()?);
        let hash = r.hash()?;
        if entries.insert(id, hash).is_some() {
            return Err(ObjectError::InvalidValue("duplicate entity id"));
        }
    }
    Ok(Tree { entries })
}

fn encode_conflict(conflict: &Conflict) -> Vec<u8> {
    let mut w = Writer::new(TAG_CONFLICT);
    w.fixed(conflict.entity.as_bytes());
    w.opt_hash(conflict.base.as_ref());
    w.opt_hash(conflict.ours.as_ref());
    w.opt_hash(conflict.theirs.as_ref());
    w.count(conflict.fields.len());
    for field in &conflict.fields {
        w.count(field.path.len());
        for segment in &field.path {
            w.str(segment);
        }
        uir::encode_opt(&mut w, field.base.as_ref());
        uir::encode_opt(&mut w, field.ours.as_ref());
        uir::encode_opt(&mut w, field.theirs.as_ref());
    }
    w.finish()
}

fn decode_conflict(r: &mut Reader<'_>) -> Result<Conflict, ObjectError> {
    let entity = EntityId::from_bytes(r.fixed()?);
    let base = r.opt_hash()?;
    let ours = r.opt_hash()?;
    let theirs = r.opt_hash()?;
    let count = r.count()?;
    let mut fields = Vec::new();
    for _ in 0..count {
        let depth = r.count()?;
        let mut path = Vec::new();
        for _ in 0..depth {
            path.push(r.str()?);
        }
        fields.push(FieldConflict {
            path,
            base: uir::decode_opt(r)?,
            ours: uir::decode_opt(r)?,
            theirs: uir::decode_opt(r)?,
        });
    }
    Ok(Conflict {
        entity,
        base,
        ours,
        theirs,
        fields,
    })
}
