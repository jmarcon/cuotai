//! Plaintext development-only token store. Never compiled into release builds.
use super::{decode, encode, validate_id, OAuthError, OAuthToken, TokenStore};
use rusqlite::{Connection, OptionalExtension};
use std::{fs, path::Path, sync::Mutex, time::Duration};

pub(super) struct SqliteDevStore(Mutex<Connection>);

fn error() -> OAuthError {
    OAuthError::Storage("SQLite de desenvolvimento indisponível. Use um arquivo privado em diretório existente, sem links simbólicos; no Unix, permissões 0600. Nenhum fallback ao Keychain foi feito.".into())
}

impl SqliteDevStore {
    pub(super) fn open(path: &Path) -> Result<Self, OAuthError> {
        if path.as_os_str().is_empty() {
            return Err(error());
        }
        // Create privately before SQLite opens it; never chmod an unrelated file.
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(error()),
        }
        let metadata = fs::symlink_metadata(path).map_err(|_| error())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(error());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.mode() & 0o077 != 0 || metadata.nlink() != 1 {
                return Err(error());
            }
        }
        // macOS /var is a symlink to /private/var. Resolve only the parent;
        // the credential file itself must still pass SQLite's NOFOLLOW check.
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let path = fs::canonicalize(parent)
            .map_err(|_| error())?
            .join(path.file_name().ok_or_else(error)?);
        let connection = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(|_| error())?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|_| error())?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=DELETE; PRAGMA secure_delete=ON;
            CREATE TABLE IF NOT EXISTS cuotai_dev_tokens (
                id TEXT PRIMARY KEY NOT NULL, payload TEXT NOT NULL
            );",
            )
            .map_err(|_| error())?;
        Ok(Self(Mutex::new(connection)))
    }
}
impl TokenStore for SqliteDevStore {
    fn load(&self, id: &str) -> Result<Option<OAuthToken>, OAuthError> {
        let id = validate_id(id)?;
        let db = self.0.lock().map_err(|_| error())?;
        let raw: Option<String> = db
            .query_row(
                "SELECT payload FROM cuotai_dev_tokens WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| error())?;
        raw.as_deref().map(decode).transpose()
    }
    fn save(&self, id: &str, token: &OAuthToken) -> Result<(), OAuthError> {
        let id = validate_id(id)?;
        let payload = encode(token)?;
        self.0.lock().map_err(|_| error())?.execute(
            "INSERT INTO cuotai_dev_tokens(id,payload) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",
            [id, payload.as_str()]).map_err(|_| error())?;
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<(), OAuthError> {
        let id = validate_id(id)?;
        self.0
            .lock()
            .map_err(|_| error())?
            .execute("DELETE FROM cuotai_dev_tokens WHERE id=?1", [id])
            .map_err(|_| error())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn token(value: &str) -> OAuthToken {
        OAuthToken {
            access_token: value.into(),
            refresh_token: Some("refresh".into()),
            expires_at: None,
            metadata: Default::default(),
        }
    }
    #[test]
    fn persists_rotation_and_deletion_across_reopen_without_mixing_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.sqlite3");
        let store = SqliteDevStore::open(&path).unwrap();
        store.save("first", &token("old")).unwrap();
        store.save("second", &token("separate")).unwrap();
        store.save("first", &token("rotated")).unwrap();
        drop(store);
        let store = SqliteDevStore::open(&path).unwrap();
        assert_eq!(
            store.load("first").unwrap().unwrap().access_token,
            "rotated"
        );
        assert_eq!(
            store.load("second").unwrap().unwrap().access_token,
            "separate"
        );
        store.delete("first").unwrap();
        drop(store);
        let store = SqliteDevStore::open(&path).unwrap();
        assert!(store.load("first").unwrap().is_none());
        assert_eq!(
            store
                .load("second")
                .unwrap()
                .unwrap()
                .refresh_token
                .as_deref(),
            Some("refresh")
        );
    }
    #[cfg(unix)]
    #[test]
    fn rejects_shared_files_and_symlinks() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.sqlite3");
        let store = SqliteDevStore::open(&path).unwrap();
        drop(store);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let link = dir.path().join("link.sqlite3");
        symlink(&path, &link).unwrap();
        assert!(SqliteDevStore::open(&link).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(SqliteDevStore::open(&path).is_err());
    }
}
