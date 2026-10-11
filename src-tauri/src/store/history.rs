//! The full message history, in an encrypted SQLite database beside the
//! session. Memory keeps the newest messages of each chat (`ChatStore`);
//! this keeps every message and the rest of the store, and is written with
//! only what changed since the last save.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Text};

use super::{ChatStore, MAX_MESSAGES_PER_CHAT, STORE_VERSION, StoredChat, StoredMessage, fold, search_text, search_words};
use crate::account::lock;

/// The database's file name in an account's folder.
pub const FILE: &str = "history.db";
/// The layout of the tables below; a newer one is refused, not guessed at.
const SCHEMA_VERSION: i32 = 1;

const SCHEMA: &str = "
    PRAGMA journal_mode = WAL;
    PRAGMA synchronous = NORMAL;
    CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS chats (id TEXT PRIMARY KEY, data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS messages (
        chat TEXT NOT NULL,
        id TEXT NOT NULL,
        ts_ms INTEGER NOT NULL,
        deleted INTEGER NOT NULL,
        folded TEXT NOT NULL,
        data TEXT NOT NULL,
        PRIMARY KEY (chat, id)
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS messages_by_time ON messages (chat, ts_ms);
";

pub struct HistoryDb {
    conn: SqliteConnection,
    saved: Saved,
}

/// What the database holds, as hashes of what was written, so a save
/// writes only what differs.
#[derive(Default)]
struct Saved {
    meta: u64,
    chats: HashMap<String, u64>,
    /// By a hash of chat and message id.
    messages: HashMap<u64, u64>,
}

/// What changed since the last save: read under the store's lock, written
/// after it is let go.
#[derive(Default)]
pub struct Changes {
    meta: Option<String>,
    chats: Vec<(String, String)>,
    /// Chats gone from memory: merged into another (a LID chat into its
    /// phone number's), whose messages move there, or dropped.
    gone: Vec<(String, Option<String>)>,
    messages: Vec<Row>,
    /// Messages that came in already too old for memory, as history does:
    /// kept only when the database doesn't have them, so a replay never
    /// overwrites what happened to them since.
    added: Vec<Row>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.meta.is_none() && self.chats.is_empty() && self.gone.is_empty() && self.messages.is_empty() && self.added.is_empty()
    }
}

struct Row {
    chat: String,
    id: String,
    ts_ms: i64,
    deleted: bool,
    folded: String,
    data: String,
}

#[derive(QueryableByName)]
struct Data {
    #[diesel(sql_type = Text)]
    data: String,
}

#[derive(QueryableByName)]
struct Keyed {
    #[diesel(sql_type = Text)]
    key: String,
    #[diesel(sql_type = Text)]
    data: String,
}

#[derive(QueryableByName)]
struct Version {
    #[diesel(sql_type = Integer)]
    user_version: i32,
}

fn hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn message_key(chat: &str, id: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    (chat, id).hash(&mut hasher);
    hasher.finish()
}

fn row(chat: &str, message: &StoredMessage) -> Option<Row> {
    let data = serde_json::to_string(message).inspect_err(|e| log::warn!("history: could not write {}: {e}", message.id)).ok()?;
    Some(Row {
        chat: chat.to_string(),
        id: message.id.clone(),
        ts_ms: message.ts_ms,
        deleted: message.deleted.is_some(),
        folded: search_text(message).map(|text| fold(&text)).unwrap_or_default(),
        data,
    })
}

fn failed(what: &str) -> impl Fn(diesel::result::Error) -> String + '_ {
    move |e| format!("history: could not {what}: {e}")
}

/// `LIKE` takes `%` and `_` as wildcards; a search means them literally.
fn like(word: &str) -> String {
    let escaped = word.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("%{escaped}%")
}

impl HistoryDb {
    /// Opens (or makes) the database at `path`, encrypted with `key`.
    pub fn open(path: &Path, key: &str) -> Result<Self, String> {
        let mut conn = SqliteConnection::establish(&path.to_string_lossy()).map_err(|e| format!("history: could not open {}: {e}", path.display()))?;
        crate::sqlcipher::unlock(&mut conn, key).map_err(|e| format!("history: could not unlock {}: {e}", path.display()))?;
        let version = diesel::sql_query("PRAGMA user_version")
            .get_result::<Version>(&mut conn)
            .map_err(failed("read its version"))?
            .user_version;
        if version > SCHEMA_VERSION {
            return Err(format!("history: {} is from a newer Omega Zap (layout {version})", path.display()));
        }
        conn.batch_execute(&format!("{SCHEMA} PRAGMA user_version = {SCHEMA_VERSION};")).map_err(failed("set up its tables"))?;
        Ok(Self { conn, saved: Saved::default() })
    }

    /// The store as it was last saved, with the newest messages of each chat
    /// in memory. `None` when nothing was ever saved here.
    pub fn load(&mut self) -> Result<Option<ChatStore>, String> {
        let meta = diesel::sql_query("SELECT value AS data FROM meta WHERE key = 'store'")
            .load::<Data>(&mut self.conn)
            .map_err(failed("read the store"))?;
        let Some(meta) = meta.into_iter().next() else {
            return Ok(None);
        };
        let mut store: ChatStore = serde_json::from_str(&meta.data).map_err(|e| format!("history: the store is unreadable: {e}"))?;
        if store.version != STORE_VERSION {
            return Err(format!("history: the store has version {}, not {STORE_VERSION}", store.version));
        }
        self.saved.meta = hash(&meta.data);
        let chats = diesel::sql_query("SELECT id AS key, data FROM chats")
            .load::<Keyed>(&mut self.conn)
            .map_err(failed("read the chats"))?;
        for Keyed { key: id, data } in chats {
            let Ok(mut chat) = serde_json::from_str::<StoredChat>(&data).inspect_err(|e| log::warn!("history: chat {id} is unreadable: {e}")) else {
                continue;
            };
            self.saved.chats.insert(id.clone(), hash(&data));
            let rows = diesel::sql_query("SELECT data FROM messages WHERE chat = ? ORDER BY ts_ms DESC, id DESC LIMIT ?")
                .bind::<Text, _>(&id)
                .bind::<BigInt, _>(MAX_MESSAGES_PER_CHAT as i64)
                .load::<Data>(&mut self.conn)
                .map_err(failed("read messages"))?;
            for Data { data } in rows.into_iter().rev() {
                match serde_json::from_str::<StoredMessage>(&data) {
                    Ok(message) => {
                        self.saved.messages.insert(message_key(&id, &message.id), hash(&data));
                        chat.messages.push(message);
                    }
                    Err(e) => log::warn!("history: a message in {id} is unreadable: {e}"),
                }
            }
            store.chats.insert(id, chat);
        }
        store.migrate_attachments();
        store.repair_unread();
        Ok(Some(store))
    }

    /// What differs between `store` and what was last saved. Taken as
    /// written: if writing fails, `forget` makes the next save write all.
    pub fn changes(&mut self, store: &mut ChatStore) -> Changes {
        let mut out = Changes::default();
        let saved = &mut self.saved;

        // The store without its chats, which have rows of their own.
        let chats = std::mem::take(&mut store.chats);
        let evicted = std::mem::take(&mut store.evicted);
        let meta = serde_json::to_string(&*store);
        store.chats = chats;
        match meta {
            Ok(meta) => {
                let h = hash(&meta);
                if h != saved.meta {
                    saved.meta = h;
                    out.meta = Some(meta);
                }
            }
            Err(e) => log::warn!("history: could not write the store: {e}"),
        }

        for (id, chat) in store.chats.iter_mut() {
            let messages = std::mem::take(&mut chat.messages);
            let data = serde_json::to_string(&*chat);
            chat.messages = messages;
            if let Ok(data) = data {
                let h = hash(&data);
                if saved.chats.get(id) != Some(&h) {
                    saved.chats.insert(id.clone(), h);
                    out.chats.push((id.clone(), data));
                }
            }
            for message in &chat.messages {
                let Some(row) = row(id, message) else { continue };
                let h = hash(&row.data);
                if saved.messages.insert(message_key(id, &message.id), h) != Some(h) {
                    out.messages.push(row);
                }
            }
        }

        for (chat, message) in evicted {
            let Some(row) = row(&chat, &message) else { continue };
            match saved.messages.remove(&message_key(&chat, &message.id)) {
                Some(h) if h == hash(&row.data) => {}
                Some(_) => out.messages.push(row),
                None => out.added.push(row),
            }
        }

        let gone: Vec<String> = saved.chats.keys().filter(|id| !store.chats.contains_key(*id)).cloned().collect();
        for id in gone {
            saved.chats.remove(&id);
            let into = store.lid_to_pn.get(&id).filter(|pn| store.chats.contains_key(*pn)).cloned();
            out.gone.push((id, into));
        }
        out
    }

    /// After a failed write: the next save writes everything again.
    pub fn forget(&mut self) {
        self.saved = Saved::default();
    }

    /// Writes changes in one transaction: all of them or none.
    pub fn write(&mut self, changes: Changes) -> Result<(), String> {
        if changes.is_empty() {
            return Ok(());
        }
        self.conn
            .transaction::<_, diesel::result::Error, _>(|conn| {
                if let Some(meta) = &changes.meta {
                    diesel::sql_query("INSERT INTO meta (key, value) VALUES ('store', ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
                        .bind::<Text, _>(meta)
                        .execute(conn)?;
                }
                for (id, into) in &changes.gone {
                    if let Some(into) = into {
                        diesel::sql_query("UPDATE OR IGNORE messages SET chat = ? WHERE chat = ?")
                            .bind::<Text, _>(into)
                            .bind::<Text, _>(id)
                            .execute(conn)?;
                    }
                    diesel::sql_query("DELETE FROM messages WHERE chat = ?").bind::<Text, _>(id).execute(conn)?;
                    diesel::sql_query("DELETE FROM chats WHERE id = ?").bind::<Text, _>(id).execute(conn)?;
                }
                for (id, data) in &changes.chats {
                    diesel::sql_query("INSERT INTO chats (id, data) VALUES (?, ?) ON CONFLICT (id) DO UPDATE SET data = excluded.data")
                        .bind::<Text, _>(id)
                        .bind::<Text, _>(data)
                        .execute(conn)?;
                }
                for (rows, verb) in [
                    (&changes.messages, "INSERT INTO messages (chat, id, ts_ms, deleted, folded, data) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (chat, id) DO UPDATE SET ts_ms = excluded.ts_ms, deleted = excluded.deleted, folded = excluded.folded, data = excluded.data"),
                    (&changes.added, "INSERT OR IGNORE INTO messages (chat, id, ts_ms, deleted, folded, data) VALUES (?, ?, ?, ?, ?, ?)"),
                ] {
                    for row in rows {
                        diesel::sql_query(verb)
                            .bind::<Text, _>(&row.chat)
                            .bind::<Text, _>(&row.id)
                            .bind::<BigInt, _>(row.ts_ms)
                            .bind::<Integer, _>(row.deleted as i32)
                            .bind::<Text, _>(&row.folded)
                            .bind::<Text, _>(&row.data)
                            .execute(conn)?;
                    }
                }
                Ok(())
            })
            .map_err(failed("save"))
    }

    /// Up to `limit` messages of a chat from before `before_ms`, oldest first.
    pub fn older(&mut self, chat: &str, before_ms: i64, limit: usize) -> Result<Vec<StoredMessage>, String> {
        let rows = diesel::sql_query("SELECT data FROM messages WHERE chat = ? AND ts_ms < ? ORDER BY ts_ms DESC, id DESC LIMIT ?")
            .bind::<Text, _>(chat)
            .bind::<BigInt, _>(before_ms)
            .bind::<BigInt, _>(limit as i64)
            .load::<Data>(&mut self.conn)
            .map_err(failed("read older messages"))?;
        Ok(rows.into_iter().rev().filter_map(|Data { data }| serde_json::from_str(&data).ok()).collect())
    }

    /// A chat's oldest message.
    pub fn oldest(&mut self, chat: &str) -> Result<Option<StoredMessage>, String> {
        let rows = diesel::sql_query("SELECT data FROM messages WHERE chat = ? ORDER BY ts_ms ASC, id ASC LIMIT 1")
            .bind::<Text, _>(chat)
            .load::<Data>(&mut self.conn)
            .map_err(failed("read the oldest message"))?;
        Ok(rows.into_iter().next().and_then(|Data { data }| serde_json::from_str(&data).ok()))
    }

    /// One message, wherever it is in the history.
    pub fn message(&mut self, chat: &str, id: &str) -> Result<Option<StoredMessage>, String> {
        let rows = diesel::sql_query("SELECT data FROM messages WHERE chat = ? AND id = ?")
            .bind::<Text, _>(chat)
            .bind::<Text, _>(id)
            .load::<Data>(&mut self.conn)
            .map_err(failed("read a message"))?;
        Ok(rows.into_iter().next().and_then(|Data { data }| serde_json::from_str(&data).ok()))
    }

    /// Messages with every one of `words` (folded) in them, newest first.
    /// Only `chat`'s, when given.
    pub fn search(&mut self, words: &[String], chat: Option<&str>, limit: usize) -> Result<Vec<(String, StoredMessage)>, String> {
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let mut sql = String::from("SELECT chat AS key, data FROM messages WHERE deleted = 0");
        if chat.is_some() {
            sql.push_str(" AND chat = ?");
        }
        for _ in words {
            sql.push_str(" AND folded LIKE ? ESCAPE '\\'");
        }
        sql.push_str(" ORDER BY ts_ms DESC LIMIT ?");
        let mut query = diesel::sql_query(sql).into_boxed();
        if let Some(chat) = chat {
            query = query.bind::<Text, _>(chat.to_string());
        }
        for word in words {
            query = query.bind::<Text, _>(like(word));
        }
        let rows = query
            .bind::<BigInt, _>(limit as i64)
            .load::<Keyed>(&mut self.conn)
            .map_err(failed("search"))?;
        Ok(rows
            .into_iter()
            .filter_map(|Keyed { key, data }| Some((key, serde_json::from_str(&data).ok()?)))
            .collect())
    }
}

/// An account's history: the database, opened when first needed, and the
/// `chats.json` it replaced, which is read once to fill it and then kept
/// aside as `chats.json.migrated`. Were the database ever to fail to open,
/// the account keeps saving to `chats.json` as it used to.
pub struct History {
    dir: PathBuf,
    purpose: &'static str,
    legacy: &'static str,
    db: Mutex<Option<HistoryDb>>,
}

/// What `chats.json` is renamed to once the database holds it.
pub const MIGRATED_SUFFIX: &str = ".migrated";

impl History {
    /// `purpose` names the key; `legacy` is the old file's name in `dir`.
    pub fn new(dir: &Path, purpose: &'static str, legacy: &'static str) -> Self {
        Self { dir: dir.to_path_buf(), purpose, legacy, db: Mutex::new(None) }
    }

    /// The files that are this history, for an account being removed.
    pub fn files(&self) -> Vec<PathBuf> {
        let migrated = format!("{}{MIGRATED_SUFFIX}", self.legacy);
        [FILE.to_string(), format!("{FILE}-wal"), format!("{FILE}-shm"), self.legacy.to_string(), migrated]
            .into_iter()
            .map(|name| self.dir.join(name))
            .collect()
    }

    /// Lets the database go, before its files are removed.
    pub fn close(&self) {
        *lock(&self.db) = None;
    }

    fn with_db<T>(&self, f: impl FnOnce(&mut HistoryDb) -> T) -> Result<T, String> {
        let mut db = lock(&self.db);
        if db.is_none() {
            *db = Some(HistoryDb::open(&self.dir.join(FILE), &crate::sqlcipher::key(self.purpose))?);
        }
        Ok(f(db.as_mut().expect("opened above")))
    }

    /// The saved store: from the database, or the first time from
    /// `chats.json`, which is then moved into the database.
    pub fn load(&self) -> Option<ChatStore> {
        let legacy = self.dir.join(self.legacy);
        let loaded = self.with_db(|db| -> Result<Option<ChatStore>, String> {
            if let Some(store) = db.load()? {
                return Ok(Some(store));
            }
            let Some(mut store) = crate::vault::get().read(&legacy).ok().and_then(|bytes| ChatStore::from_json(&bytes)) else {
                return Ok(None);
            };
            let changes = db.changes(&mut store);
            if let Err(e) = db.write(changes) {
                db.forget();
                log::warn!("{e}; keeping {}", legacy.display());
                return Ok(Some(store));
            }
            let mut aside = legacy.clone().into_os_string();
            aside.push(MIGRATED_SUFFIX);
            if let Err(e) = std::fs::rename(&legacy, &aside) {
                log::warn!("history: could not set {} aside: {e}", legacy.display());
            }
            log::info!("history: moved {} into {FILE}", legacy.display());
            Ok(Some(store))
        });
        match loaded {
            Ok(Ok(store)) => store,
            Ok(Err(e)) | Err(e) => {
                log::warn!("{e}; reading {} instead", legacy.display());
                crate::vault::get().read(&legacy).ok().and_then(|bytes| ChatStore::from_json(&bytes))
            }
        }
    }

    /// Writes what changed in `store` since the last save.
    pub fn save(&self, store: &Mutex<ChatStore>) {
        let saved = self.with_db(|db| {
            let changes = db.changes(&mut lock(store));
            db.write(changes).inspect_err(|_| db.forget())
        });
        match saved {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("{e}"),
            // No database: the whole store goes to the old file, as before.
            Err(e) => {
                log::warn!("{e}");
                match lock(store).to_json() {
                    Ok(bytes) => {
                        if let Err(e) = crate::vault::get().write(&self.dir.join(self.legacy), &bytes) {
                            log::warn!("could not save chats: {e}");
                        }
                    }
                    Err(e) => log::warn!("could not serialize chats: {e}"),
                }
            }
        }
    }

    /// Up to `limit` messages of `chat` from before `before_ms`, oldest first.
    pub fn older(&self, chat: &str, before_ms: i64, limit: usize) -> Vec<StoredMessage> {
        self.with_db(|db| db.older(chat, before_ms, limit))
            .and_then(|found| found)
            .inspect_err(|e| log::warn!("{e}"))
            .unwrap_or_default()
    }

    /// A chat's oldest saved message.
    pub fn oldest(&self, chat: &str) -> Option<StoredMessage> {
        self.with_db(|db| db.oldest(chat))
            .and_then(|found| found)
            .inspect_err(|e| log::warn!("{e}"))
            .ok()
            .flatten()
    }

    /// One saved message, wherever it is in the history.
    pub fn message(&self, chat: &str, id: &str) -> Option<StoredMessage> {
        self.with_db(|db| db.message(chat, id))
            .and_then(|found| found)
            .inspect_err(|e| log::warn!("{e}"))
            .ok()
            .flatten()
    }

    /// Saved messages with every word of `query` in them, newest first;
    /// only `chat`'s when given.
    pub fn search(&self, query: &str, chat: Option<&str>, limit: usize) -> Vec<(String, StoredMessage)> {
        let words = search_words(query);
        self.with_db(|db| db.search(&words, chat, limit))
            .and_then(|found| found)
            .inspect_err(|e| log::warn!("{e}"))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Content, Delivery, Incoming};
    use super::*;

    fn open(dir: &Path) -> HistoryDb {
        HistoryDb::open(&dir.join(FILE), "x'2DD29CA851E7B56E4697B0E1F08507293D761A05CE4D1B628663F411A8086D99'").unwrap()
    }

    fn incoming(chat: &str, id: &str, ts: i64, text: &str) -> Incoming {
        Incoming {
            chat: chat.into(),
            is_group: false,
            sender: Some(chat.into()),
            push_name: None,
            lid_pn_pairs: Vec::new(),
            message: StoredMessage {
                id: id.into(),
                from_me: false,
                ts_ms: ts,
                author_jid: None,
                content: Content::text(text, None),
                status: None::<Delivery>,
                reactions: Vec::new(),
                wire: None,
                read_by: Vec::new(),
                starred: false,
                deleted: None,
            },
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("omega-zap-history-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn keeps_every_message_and_memory_only_the_newest() {
        let dir = temp_dir("all");
        let chat = "351900000000@s.whatsapp.net";
        let mut store = ChatStore::default();
        let mut db = open(&dir);
        let total = MAX_MESSAGES_PER_CHAT + 50;
        for n in 0..total {
            store.ingest(incoming(chat, &format!("m{n:04}"), n as i64, &format!("message {n}")), false);
            // Saved now and then, as the app does, so some leave memory unsaved.
            if n % 97 == 0 {
                let changes = db.changes(&mut store);
                db.write(changes).unwrap();
            }
        }
        let changes = db.changes(&mut store);
        db.write(changes).unwrap();
        // Nothing changed: nothing to write.
        assert!(db.changes(&mut store).is_empty());
        drop(db);

        let mut db = open(&dir);
        let loaded = db.load().unwrap().expect("saved");
        assert_eq!(loaded.messages(chat).len(), MAX_MESSAGES_PER_CHAT);
        let older = db.older(chat, 50, 1000).unwrap();
        assert_eq!(older.len(), 50);
        assert_eq!(older[0].id, "m0000");
        let found = db.search(&["message".into(), "7".into()], Some(chat), 1000).unwrap();
        assert!(found.iter().any(|(_, m)| m.id == "m0007"));
        assert!(found.iter().all(|(_, m)| m.content.text.as_deref().unwrap().contains('7')));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_replayed_old_message_does_not_overwrite_the_saved_one() {
        let dir = temp_dir("replay");
        let chat = "351900000000@s.whatsapp.net";
        let mut store = ChatStore::default();
        let mut db = open(&dir);
        for n in 0..=MAX_MESSAGES_PER_CHAT {
            store.ingest(incoming(chat, &format!("m{n:04}"), 10 + n as i64, "first"), false);
        }
        let changes = db.changes(&mut store);
        db.write(changes).unwrap();
        // m0000 left memory; history brings it again, as it was sent.
        store.ingest(incoming(chat, "m0000", 10, "replayed"), false);
        let changes = db.changes(&mut store);
        db.write(changes).unwrap();
        let kept = db.message(chat, "m0000").unwrap().unwrap();
        assert_eq!(kept.content.text.as_deref(), Some("first"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lid_chat_merged_into_its_phone_number_moves_its_history() {
        let dir = temp_dir("merge");
        let (lid, pn) = ("111@lid", "351900000000@s.whatsapp.net");
        let mut store = ChatStore::default();
        let mut db = open(&dir);
        for n in 0..=MAX_MESSAGES_PER_CHAT {
            store.ingest(incoming(lid, &format!("m{n:04}"), n as i64, "hi"), false);
        }
        let changes = db.changes(&mut store);
        db.write(changes).unwrap();
        store.learn_mapping(lid, pn);
        let changes = db.changes(&mut store);
        db.write(changes).unwrap();
        assert!(db.message(lid, "m0000").unwrap().is_none());
        assert!(db.message(pn, "m0000").unwrap().is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

