//! The session databases, encrypted with SQLCipher under keys derived from
//! the vault. A database from before encryption is converted once, the first
//! time it is opened.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::Integer;

use crate::vault;

/// What a database is opened with: `PRAGMA key` with the raw key, which
/// skips SQLCipher's passphrase derivation and so costs nothing per connection.
pub fn key(purpose: &str) -> String {
    vault::sqlcipher_key(&vault::get().derive(purpose))
}

/// Keys a fresh connection and checks the key opens the database.
pub fn unlock(conn: &mut SqliteConnection, key: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    conn.batch_execute(&format!("PRAGMA key = \"{key}\"; SELECT count(*) FROM sqlite_master;"))?;
    Ok(())
}

#[derive(QueryableByName)]
struct UserVersion {
    #[diesel(sql_type = Integer)]
    user_version: i32,
}

/// Rewrites a plain SQLite database at `path` as an encrypted one. Does
/// nothing if there is none, or if it is encrypted already.
pub fn encrypt_in_place(path: &Path, key: &str) -> Result<(), String> {
    let mut head = [0u8; 16];
    match File::open(path) {
        Ok(mut file) => {
            if file.read_exact(&mut head).is_err() || &head != b"SQLite format 3\0" {
                return Ok(());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".encrypting");
    let tmp = std::path::PathBuf::from(tmp);
    let _ = fs::remove_file(&tmp);
    let failed = |e: &dyn std::fmt::Display| format!("could not encrypt {}: {e}", path.display());
    {
        let mut conn = SqliteConnection::establish(&path.to_string_lossy()).map_err(|e| failed(&e))?;
        let version = diesel::sql_query("PRAGMA user_version")
            .get_result::<UserVersion>(&mut conn)
            .map_err(|e| failed(&e))?
            .user_version;
        let target = tmp.to_string_lossy().replace('\'', "''");
        conn.batch_execute(&format!(
            "PRAGMA wal_checkpoint(TRUNCATE);
             ATTACH DATABASE '{target}' AS sealed KEY \"{key}\";
             SELECT sqlcipher_export('sealed');
             PRAGMA sealed.user_version = {version};
             DETACH DATABASE sealed;"
        ))
        .map_err(|e| {
            let _ = fs::remove_file(&tmp);
            failed(&e)
        })?;
    }
    // The journal belonged to the plain file and was folded into it above.
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut side = path.as_os_str().to_owned();
        side.push(suffix);
        let _ = fs::remove_file(side);
    }
    fs::rename(&tmp, path).map_err(|e| failed(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_database_is_encrypted_and_keeps_its_rows() {
        let dir = std::env::temp_dir().join(format!("oz-sqlcipher-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        let _ = fs::remove_file(&path);
        let url = path.to_string_lossy().to_string();
        {
            let mut conn = SqliteConnection::establish(&url).unwrap();
            conn.batch_execute(
                "PRAGMA journal_mode=WAL; CREATE TABLE t(v TEXT); INSERT INTO t VALUES ('secret-row'); PRAGMA user_version = 7;",
            )
            .unwrap();
        }
        let key = vault::sqlcipher_key(&[3; 32]);
        encrypt_in_place(&path, &key).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(!bytes.starts_with(b"SQLite format 3"));
        assert!(!bytes.windows(10).any(|w| w == b"secret-row"));
        // Running again on the encrypted file leaves it alone.
        encrypt_in_place(&path, &key).unwrap();

        let mut conn = SqliteConnection::establish(&url).unwrap();
        unlock(&mut conn, &key).unwrap();
        let version = diesel::sql_query("PRAGMA user_version").get_result::<UserVersion>(&mut conn).unwrap();
        assert_eq!(version.user_version, 7);
        conn.batch_execute("INSERT INTO t VALUES ('x')").unwrap();

        let mut wrong = SqliteConnection::establish(&url).unwrap();
        assert!(unlock(&mut wrong, &vault::sqlcipher_key(&[4; 32])).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
