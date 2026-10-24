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
use crate::oplog::OpLog;
use crate::sign::Signature;
use crate::uir::{self, UirNode};

const TAG_BLOB: u8 = 1;
const TAG_ENTITY: u8 = 2;
const TAG_TREE: u8 = 3;
const TAG_COMMIT: u8 = 4;
const TAG_CONFLICT: u8 = 5;
const TAG_REVIEW: u8 = 6;
const TAG_TREE_ROOT: u8 = 7;
const TAG_ANNOTATED_TAG: u8 = 8;
const TAG_OPLOG: u8 = 9;

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
/// A large snapshot, split into shards. Each shard is a [`Tree`] holding the
/// entities whose id hashes to the same first byte, and this root maps that byte to
/// the shard's hash. A shard that did not change keeps its hash, so an edit
/// stores only the shards it touches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRoot {
    /// The first byte of an entity id's hash, to the hash of the shard holding it.
    pub shards: BTreeMap<u8, ContentHash>,
}

/// A snapshot of entities: each entity id maps to the hash of its current version.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tree {
    /// Entity id to the hash of that entity's current version.
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
    /// The hash of the snapshot this commit records.
    pub tree_hash: ContentHash,
    /// The commits this one follows. Two for a merge commit, none for a root.
    pub parents: Vec<ContentHash>,
    /// Who made the commit, as the caller names them. This is a label, not a
    /// proof of identity: nothing checks it against the signer. The signature
    /// (see [`Commit::signature`]) is what ties a commit to a key, and the author
    /// may differ from the signer, for example when a service signs for a user.
    pub author: ActorId,
    /// When the commit was made, from the repository's clock.
    pub timestamp: u64,
    /// The commit message.
    pub message: String,
    /// The signature, if the repository signs its commits.
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
    /// The conflicted entity.
    pub entity: EntityId,
    /// The common ancestor's version, or `None` if the entity was not there.
    pub base: Option<ContentHash>,
    /// Our version, or `None` if we deleted the entity.
    pub ours: Option<ContentHash>,
    /// Their version, or `None` if they deleted the entity.
    pub theirs: Option<ContentHash>,
    /// The fields that conflict. Empty for a whole-entity conflict.
    pub fields: Vec<FieldConflict>,
}

/// One field that changed differently on both sides. An empty `path` means
/// the whole entity, which happens for delete-versus-modify.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldConflict {
    /// The path from the entity root to the conflicting field.
    pub path: Vec<String>,
    /// The field's value in the common ancestor.
    pub base: Option<UirNode>,
    /// The field's value on our side.
    pub ours: Option<UirNode>,
    /// The field's value on their side.
    pub theirs: Option<UirNode>,
}

/// Any object the store holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Object {
    /// Raw bytes that are not an entity.
    Blob(Vec<u8>),
    /// An entity's value, as a UIR tree.
    Entity(UirNode),
    /// A snapshot of entities.
    Tree(Tree),
    /// A large snapshot split into shards. See [`TreeRoot`].
    TreeRoot(TreeRoot),
    /// A snapshot with its history.
    Commit(Commit),
    /// An unresolved merge conflict.
    Conflict(Conflict),
    /// A signed verdict on a commit.
    Review(Review),
    /// An annotated, optionally signed tag. See [`AnnotatedTag`].
    AnnotatedTag(AnnotatedTag),
    /// A saved live-collaboration op log. See [`OpLog`].
    OpLog(OpLog),
}

impl Object {
    /// A short name for the kind, used in error messages.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Blob(_) => "blob",
            Self::Entity(_) => "entity",
            Self::Tree(_) => "tree",
            Self::TreeRoot(_) => "tree-root",
            Self::Commit(_) => "commit",
            Self::Conflict(_) => "conflict",
            Self::Review(_) => "review",
            Self::AnnotatedTag(_) => "annotated tag",
            Self::OpLog(_) => "op log",
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
            Self::TreeRoot(root) => encode_tree_root(root),
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
            Self::Review(review) => encode_review(review, true),
            Self::AnnotatedTag(tag) => encode_annotated_tag(tag, true),
            Self::OpLog(log) => {
                let mut w = Writer::new(TAG_OPLOG);
                log.encode_into(&mut w);
                w.finish()
            }
        }
    }

    /// Decodes canonical bytes produced by [`Object::encode`].
    pub fn decode(data: &[u8]) -> Result<Self, ObjectError> {
        let (tag, mut r) = Reader::open(data)?;
        let object = match tag {
            TAG_BLOB => Self::Blob(r.bytes()?),
            TAG_ENTITY => Self::Entity(UirNode::decode(&mut r)?),
            TAG_TREE => Self::Tree(decode_tree(&mut r)?),
            TAG_TREE_ROOT => Self::TreeRoot(decode_tree_root(&mut r)?),
            TAG_COMMIT => Self::Commit(decode_commit(&mut r)?),
            TAG_CONFLICT => Self::Conflict(decode_conflict(&mut r)?),
            TAG_REVIEW => Self::Review(decode_review(&mut r)?),
            TAG_ANNOTATED_TAG => Self::AnnotatedTag(decode_annotated_tag(&mut r)?),
            TAG_OPLOG => Self::OpLog(OpLog::decode_from(&mut r)?),
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

/// What a reviewer concluded about a commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The commit is good as it is.
    Approve,
    /// The commit needs changes before it should be accepted.
    RequestChanges,
    /// A remark that does not decide anything.
    Comment,
}

impl Verdict {
    fn to_byte(self) -> u8 {
        match self {
            Self::Approve => 0,
            Self::RequestChanges => 1,
            Self::Comment => 2,
        }
    }

    fn from_byte(byte: u8) -> Result<Self, ObjectError> {
        match byte {
            0 => Ok(Self::Approve),
            1 => Ok(Self::RequestChanges),
            2 => Ok(Self::Comment),
            _ => Err(ObjectError::InvalidValue("verdict")),
        }
    }
}

/// A signed verdict on one commit. Stored in the CAS, and listed under the
/// `reviews/` refs so that gc and fsck see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    /// The commit being reviewed.
    pub commit: ContentHash,
    /// The verdict.
    pub verdict: Verdict,
    /// Who wrote the review. As with [`Commit::author`], a label, not proof.
    pub author: ActorId,
    /// When the review was written, from the repository's clock.
    pub timestamp: u64,
    /// Free text explaining the verdict.
    pub note: String,
    /// The reviewer's signature over [`Review::signing_bytes`].
    pub signature: Option<Signature>,
}

impl Review {
    /// The bytes the signature covers: the review encoded without its signature.
    pub fn signing_bytes(&self) -> Vec<u8> {
        encode_review(self, false)
    }

    /// Whether the signature is present and verifies against the review's
    /// contents. It says nothing about whether the signer is trusted.
    pub fn is_valid(&self) -> bool {
        self.signature
            .as_ref()
            .is_some_and(|signature| crate::sign::verify(signature, &self.signing_bytes()))
    }
}

fn encode_review(review: &Review, with_signature: bool) -> Vec<u8> {
    let mut w = Writer::new(TAG_REVIEW);
    w.fixed(review.commit.as_bytes());
    w.u8(review.verdict.to_byte());
    w.fixed(review.author.as_bytes());
    w.u64(review.timestamp);
    w.str(&review.note);
    match (&review.signature, with_signature) {
        (Some(signature), true) => {
            w.u8(1);
            w.fixed(&signature.signer);
            w.fixed(&signature.bytes);
        }
        _ => w.u8(0),
    }
    w.finish()
}

fn decode_review(r: &mut Reader<'_>) -> Result<Review, ObjectError> {
    let commit = r.hash()?;
    let verdict = Verdict::from_byte(r.u8()?)?;
    let author = ActorId::from_bytes(r.fixed()?);
    let timestamp = r.u64()?;
    let note = r.str()?;
    let signature = match r.u8()? {
        0 => None,
        1 => Some(Signature {
            signer: r.fixed()?,
            bytes: r.fixed()?,
        }),
        _ => return Err(ObjectError::InvalidValue("signature flag")),
    };
    Ok(Review {
        commit,
        verdict,
        author,
        timestamp,
        note,
        signature,
    })
}

/// An annotated tag: a name, a message and a signed reference to a commit. It is
/// stored in the CAS, and its ref under `tags/` points here instead of at the
/// commit itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotatedTag {
    /// The commit being tagged.
    pub commit: ContentHash,
    /// The tag's name, as it appears in its ref.
    pub name: String,
    /// Who created the tag. As with [`Commit::author`], a label, not proof.
    pub author: ActorId,
    /// When the tag was created, from the repository's clock.
    pub timestamp: u64,
    /// The tag's message.
    pub message: String,
    /// The creator's signature over [`AnnotatedTag::signing_bytes`].
    pub signature: Option<Signature>,
}

impl AnnotatedTag {
    /// The bytes the signature covers: the tag encoded without its signature.
    pub fn signing_bytes(&self) -> Vec<u8> {
        encode_annotated_tag(self, false)
    }

    /// Whether the signature is present and verifies against the tag's contents.
    /// It says nothing about whether the signer is trusted.
    pub fn is_valid(&self) -> bool {
        self.signature
            .as_ref()
            .is_some_and(|signature| crate::sign::verify(signature, &self.signing_bytes()))
    }
}

fn encode_annotated_tag(tag: &AnnotatedTag, with_signature: bool) -> Vec<u8> {
    let mut w = Writer::new(TAG_ANNOTATED_TAG);
    w.fixed(tag.commit.as_bytes());
    w.str(&tag.name);
    w.fixed(tag.author.as_bytes());
    w.u64(tag.timestamp);
    w.str(&tag.message);
    match (&tag.signature, with_signature) {
        (Some(signature), true) => {
            w.u8(1);
            w.fixed(&signature.signer);
            w.fixed(&signature.bytes);
        }
        _ => w.u8(0),
    }
    w.finish()
}

fn decode_annotated_tag(r: &mut Reader<'_>) -> Result<AnnotatedTag, ObjectError> {
    let commit = r.hash()?;
    let name = r.str()?;
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
    Ok(AnnotatedTag {
        commit,
        name,
        author,
        timestamp,
        message,
        signature,
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

fn encode_tree_root(root: &TreeRoot) -> Vec<u8> {
    let mut w = Writer::new(TAG_TREE_ROOT);
    w.count(root.shards.len());
    for (prefix, hash) in &root.shards {
        w.u8(*prefix);
        w.fixed(hash.as_bytes());
    }
    w.finish()
}

fn decode_tree_root(r: &mut Reader<'_>) -> Result<TreeRoot, ObjectError> {
    let count = r.count()?;
    let mut shards = BTreeMap::new();
    let mut previous: Option<u8> = None;
    for _ in 0..count {
        let prefix = r.u8()?;
        // Prefixes must be strictly ascending, so each root has one encoding.
        if previous.is_some_and(|last| prefix <= last) {
            return Err(ObjectError::InvalidValue("shard prefixes out of order"));
        }
        previous = Some(prefix);
        shards.insert(prefix, r.hash()?);
    }
    Ok(TreeRoot { shards })
}

fn decode_tree(r: &mut Reader<'_>) -> Result<Tree, ObjectError> {
    let count = r.count()?;
    let mut entries = BTreeMap::new();
    let mut previous: Option<EntityId> = None;
    for _ in 0..count {
        let id = EntityId::from_bytes(r.fixed()?);
        let hash = r.hash()?;
        // Ids must be strictly ascending, so each tree has one encoding.
        if previous.is_some_and(|last| id <= last) {
            return Err(ObjectError::InvalidValue("entity ids out of order"));
        }
        previous = Some(id);
        entries.insert(id, hash);
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
