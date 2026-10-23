//! Signed commits. Each commit carries an Ed25519 signature over its canonical
//! bytes. The signature is checked whenever a commit is loaded, so a commit
//! that was altered or forged is refused rather than read.
//!
//! Run with `cargo run --example signed_history`.

use std::env;
use std::fs;

use tpt_version::dag;
use tpt_version::object::{Commit, ContentHash, Object};
use tpt_version::repo::SystemClock;
use tpt_version::sign::Signature;
use tpt_version::store::{CasStore, FileSystemStore, RefStore};
use tpt_version::{ActorId, Ed25519Signer, RepoError, Repository};

fn short(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn main() -> Result<(), RepoError> {
    let root = env::temp_dir().join("tpt-version-signed-history");
    let _ = fs::remove_dir_all(&root);
    let author = ActorId::from_bytes([1; 16]);

    // Commit with a signing key. Commits with no entity changes are allowed.
    let signer = Ed25519Signer::from_seed([7; 32]);
    let mut repo = Repository::open(FileSystemStore::open(&root)?, SystemClock, "main")?
        .with_signer(Box::new(signer));
    for message in ["first", "second", "third"] {
        repo.commit(message, &author)?;
    }
    drop(repo);

    // Reopening verifies every signature on the way in.
    let repo = Repository::open(FileSystemStore::open(&root)?, SystemClock, "main")?;
    println!("history on main, signatures checked on load:");
    for commit in repo.log("main")? {
        match &commit.signature {
            Some(signature) => println!(
                "  {:<8} signed by {}...",
                commit.message,
                short(&signature.signer)
            ),
            None => println!("  {:<8} unsigned", commit.message),
        }
    }
    drop(repo);

    // Forge a commit that claims to be the new head, signed with a key it does
    // not have. The signature is a placeholder, so verification must fail.
    let mut store = FileSystemStore::open(&root)?;
    let head = store.read_ref("main")?.expect("main exists");
    let honest = dag::load_commit(&store, &head)?;
    let forged = Commit {
        tree_hash: honest.tree_hash,
        parents: vec![head],
        author,
        timestamp: honest.timestamp + 1,
        message: "forged".into(),
        signature: Some(Signature {
            signer: [9; 32],
            bytes: [0; 64],
        }),
    };
    let data = Object::Commit(forged).encode();
    let forged_hash = ContentHash::of(&data);
    store.put(forged_hash, &data)?;
    store.write_ref("main", forged_hash)?;

    match Repository::open(FileSystemStore::open(&root)?, SystemClock, "main") {
        Err(RepoError::InvalidSignature(hash)) => {
            println!("refused the forged commit {hash}: its signature does not verify");
        }
        Err(other) => return Err(other),
        Ok(_) => println!("unexpected: the forged commit loaded"),
    }

    fs::remove_dir_all(&root)
        .map_err(|error| RepoError::Store(tpt_version::StoreError::Backend(error.to_string())))?;
    Ok(())
}
