//! Encryption of what is kept on disk: the chat stores, downloaded files,
//! profile pictures and the session databases.
//!
//! One random key, kept in the desktop's keyring or the macOS Keychain (or,
//! where there is neither, in a file only we can read), and from it one key per file. A file is a header
//! (magic and a random salt) and then 64 KiB chunks, each sealed with
//! AES-256-GCM under its index, the last one marked as last: chunks cannot be
//! reordered, dropped or cut short unnoticed, and a part of a video is read
//! without decrypting the rest.
//!
//! Files written before encryption are still read as they are, until the
//! migration that runs at startup has sealed them.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::hkdf;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

const MAGIC: &[u8; 4] = b"OZV1";
const SALT_LEN: usize = 16;
const HEADER: usize = MAGIC.len() + SALT_LEN;
const CHUNK: usize = 64 * 1024;
const TAG: usize = 16;
const SEALED_CHUNK: usize = CHUNK + TAG;

/// Says which key the data directory is sealed with, and checks it.
const MARKER_FILE: &str = "vault.json";
/// The key itself, only where there is no keyring.
const KEY_FILE: &str = "vault.key";
const CHECK: &[u8] = b"omega-zap";

static VAULT: OnceLock<Vault> = OnceLock::new();

pub struct Vault {
    master: [u8; 32],
}

/// The vault opened at startup.
pub fn get() -> &'static Vault {
    VAULT.get().expect("the vault is opened before anything reads or writes")
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
enum KeyStore {
    Keyring,
    File,
}

#[derive(Serialize, Deserialize)]
struct Marker {
    store: KeyStore,
    /// `CHECK`, sealed: tells a wrong key from a right one.
    check: String,
    /// Whether everything written before encryption has been sealed.
    #[serde(default)]
    migrated: bool,
}

/// Opens the vault of `dir`, making one the first time.
pub fn open(dir: &Path) -> Result<&'static Vault, String> {
    crate::account::secure_dir(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let marker_path = dir.join(MARKER_FILE);
    let vault = match fs::read(&marker_path) {
        Ok(bytes) => {
            let marker: Marker =
                serde_json::from_slice(&bytes).map_err(|e| format!("{MARKER_FILE} is damaged: {e}"))?;
            let key = match marker.store {
                KeyStore::Keyring => keyring::load()?,
                KeyStore::File => read_key_file(&dir.join(KEY_FILE))?,
            };
            let vault = Vault { master: key };
            let check = BASE64.decode(&marker.check).map_err(|e| format!("{MARKER_FILE} is damaged: {e}"))?;
            if vault.open_bytes(&check).ok().as_deref() != Some(CHECK) {
                return Err("The storage key does not open this data. Was the keyring reset?".into());
            }
            if marker.store == KeyStore::File && keyring::TAKES_OVER_FILE {
                move_to_keyring(dir, &marker_path, marker, &key);
            }
            vault
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let mut key = [0u8; 32];
            SystemRandom::new().fill(&mut key).map_err(|_| "no randomness for a key")?;
            let store = match keyring::save(&key) {
                Ok(()) => KeyStore::Keyring,
                Err(e) => {
                    log::warn!("the keyring is not available ({e}); the storage key is kept in a file instead");
                    write_private(&dir.join(KEY_FILE), hex(&key).as_bytes())
                        .map_err(|e| format!("could not save the storage key: {e}"))?;
                    KeyStore::File
                }
            };
            let vault = Vault { master: key };
            let marker = Marker { store, check: BASE64.encode(vault.seal_bytes(CHECK)), migrated: false };
            write_private(&marker_path, &serde_json::to_vec(&marker).expect("the marker serializes"))
                .map_err(|e| format!("could not save {MARKER_FILE}: {e}"))?;
            vault
        }
        Err(e) => return Err(format!("could not read {MARKER_FILE}: {e}")),
    };
    Ok(VAULT.get_or_init(|| vault))
}

/// Moves a key kept in a file into the keyring, where the file was only a
/// fallback (macOS builds made before Keychain support). The file goes last,
/// so a failure halfway leaves a key that still opens the data.
fn move_to_keyring(dir: &Path, marker_path: &Path, marker: Marker, key: &[u8; 32]) {
    if let Err(e) = keyring::save(key) {
        log::warn!("the storage key stays in a file: {e}");
        return;
    }
    let marker = Marker { store: KeyStore::Keyring, ..marker };
    if let Err(e) = write_private(marker_path, &serde_json::to_vec(&marker).expect("the marker serializes")) {
        log::warn!("could not save {MARKER_FILE}: {e}");
        return;
    }
    match fs::remove_file(dir.join(KEY_FILE)) {
        Ok(()) => log::info!("the storage key moved into the keyring"),
        Err(e) => log::warn!("the storage key is in the keyring, but {KEY_FILE} stays: {e}"),
    }
}

/// Seals, in the background, every file written before encryption under
/// the given directories (and files), once.
pub fn migrate(dir: PathBuf, targets: Vec<PathBuf>, plain_caches: Vec<PathBuf>) {
    let marker_path = dir.join(MARKER_FILE);
    let Some(mut marker) = fs::read(&marker_path).ok().and_then(|b| serde_json::from_slice::<Marker>(&b).ok()) else {
        return;
    };
    if marker.migrated {
        return;
    }
    // Copies the webview made of pictures and videos it was shown. Removed
    // here, before there is a webview to use them.
    let mut failed = 0usize;
    for cache in plain_caches {
        if cache.exists()
            && let Err(e) = fs::remove_dir_all(&cache)
        {
            log::warn!("could not clear {}: {e}", cache.display());
            failed += 1;
        }
    }
    std::thread::spawn(move || {
        let vault = get();
        for target in &targets {
            failed += vault.seal_tree(target);
        }
        if failed > 0 {
            return log::warn!("{failed} files could not be encrypted; trying again at the next start");
        }
        marker.migrated = true;
        if let Err(e) = write_private(&marker_path, &serde_json::to_vec(&marker).expect("the marker serializes")) {
            log::warn!("could not save {MARKER_FILE}: {e}");
        }
    });
}

fn file_key(master: &[u8; 32], salt: &[u8]) -> LessSafeKey {
    let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, salt).extract(master);
    let okm = prk.expand(&[b"omega-zap file v1"], &AES_256_GCM).expect("an AES key is a valid HKDF length");
    LessSafeKey::new(UnboundKey::from(okm))
}

impl Vault {
    fn file_key(&self, salt: &[u8]) -> LessSafeKey {
        file_key(&self.master, salt)
    }

    /// A key for something other than files, such as a database.
    pub fn derive(&self, purpose: &str) -> [u8; 32] {
        struct Len;
        impl hkdf::KeyType for Len {
            fn len(&self) -> usize {
                32
            }
        }
        let mut out = [0u8; 32];
        hkdf::Salt::new(hkdf::HKDF_SHA256, b"omega-zap")
            .extract(&self.master)
            .expand(&[purpose.as_bytes()], Len)
            .and_then(|okm| okm.fill(&mut out))
            .expect("32 bytes is a valid HKDF length");
        out
    }

    pub fn seal_bytes(&self, plain: &[u8]) -> Vec<u8> {
        let mut writer = Writer::new(self, io::Cursor::new(Vec::with_capacity(sealed_len(plain.len()))))
            .expect("writing to memory does not fail");
        writer.write_all(plain).expect("writing to memory does not fail");
        writer.finish().expect("writing to memory does not fail").into_inner()
    }

    pub fn open_bytes(&self, data: &[u8]) -> io::Result<Vec<u8>> {
        if !is_sealed(data) {
            return Ok(data.to_vec());
        }
        let key = self.file_key(&data[MAGIC.len()..HEADER]);
        let body = &data[HEADER..];
        let chunks = chunk_count(body.len())?;
        let mut out = Vec::with_capacity(body.len() - chunks * TAG);
        for (index, sealed) in body.chunks(SEALED_CHUNK).enumerate() {
            let mut buf = sealed.to_vec();
            out.extend_from_slice(open_chunk(&key, index, index + 1 == chunks, &mut buf)?);
        }
        Ok(out)
    }

    /// Reads a file whole.
    pub fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.open_bytes(&fs::read(path)?)
    }

    /// Writes a file whole, in its place only once it is complete.
    pub fn write(&self, path: &Path, plain: &[u8]) -> io::Result<()> {
        let tmp = temp_beside(path);
        let result = (|| {
            let mut writer = Writer::create(self, &tmp)?;
            writer.write_all(plain)?;
            writer.finish()?;
            fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    /// The length of a file's contents.
    pub fn len(&self, path: &Path) -> io::Result<u64> {
        let mut file = File::open(path)?;
        let size = file.metadata()?.len();
        let mut magic = [0u8; MAGIC.len()];
        if size < HEADER as u64 || file.read_exact(&mut magic).is_err() || &magic != MAGIC {
            return Ok(size);
        }
        let body = (size - HEADER as u64) as usize;
        Ok((body - chunk_count(body)? * TAG) as u64)
    }

    /// Reads bytes `start..=end` of a file's contents, decrypting only the
    /// chunks they are in.
    pub fn read_range(&self, path: &Path, start: u64, end: u64) -> io::Result<Vec<u8>> {
        let mut file = File::open(path)?;
        let size = file.metadata()?.len();
        let mut header = [0u8; HEADER];
        let sealed = size >= HEADER as u64 && file.read_exact(&mut header).is_ok() && &header[..MAGIC.len()] == MAGIC;
        if !sealed {
            let len = (end - start + 1) as usize;
            let mut out = vec![0u8; len];
            file.seek(SeekFrom::Start(start))?;
            file.read_exact(&mut out)?;
            return Ok(out);
        }
        let key = self.file_key(&header[MAGIC.len()..]);
        let chunks = chunk_count((size - HEADER as u64) as usize)?;
        let (first, last) = ((start / CHUNK as u64) as usize, (end / CHUNK as u64) as usize);
        let mut out = Vec::with_capacity((end - start + 1) as usize);
        let mut buf = vec![0u8; SEALED_CHUNK];
        file.seek(SeekFrom::Start((HEADER + first * SEALED_CHUNK) as u64))?;
        for index in first..=last.min(chunks - 1) {
            let n = read_up_to(&mut file, &mut buf)?;
            let plain = open_chunk(&key, index, index + 1 == chunks, &mut buf[..n])?;
            let base = (index * CHUNK) as u64;
            let from = start.saturating_sub(base) as usize;
            let to = ((end - base + 1) as usize).min(plain.len());
            out.extend_from_slice(&plain[from.min(to)..to]);
        }
        Ok(out)
    }

    /// A decrypted copy of a file in the private runtime directory, for an
    /// app that needs a path, such as a viewer or the notification daemon.
    /// It lives in memory (tmpfs) and is gone after a restart.
    pub fn export(&self, path: &Path, name: &str) -> io::Result<PathBuf> {
        let dir = runtime_dir()?;
        let out = dir.join(name.replace(['/', '\\'], "_"));
        let mut writer = File::create(&out)?;
        set_private(&out)?;
        writer.write_all(&self.read(path)?)?;
        Ok(out)
    }

    /// Seals a file in place, streaming, if it is not sealed yet.
    pub fn seal_file(&self, path: &Path) -> io::Result<()> {
        let mut src = File::open(path)?;
        let mut magic = [0u8; MAGIC.len()];
        if src.read_exact(&mut magic).is_ok() && &magic == MAGIC {
            return Ok(());
        }
        src.rewind()?;
        let tmp = temp_beside(path);
        let result = (|| {
            let mut writer = Writer::create(self, &tmp)?;
            io::copy(&mut src, &mut writer)?;
            writer.finish()?;
            fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    /// Moves a plain file another program wrote into `dest`, sealed.
    pub fn import(&self, src: &Path, dest: &Path) -> io::Result<()> {
        let tmp = temp_beside(dest);
        let result = (|| {
            let mut plain = File::open(src)?;
            let mut writer = Writer::create(self, &tmp)?;
            io::copy(&mut plain, &mut writer)?;
            writer.finish()?;
            fs::rename(&tmp, dest)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        let _ = fs::remove_file(src);
        result
    }

    /// Seals every file under `path`. Returns how many could not be.
    fn seal_tree(&self, path: &Path) -> usize {
        let Ok(meta) = fs::symlink_metadata(path) else {
            return 0;
        };
        if meta.is_file() {
            let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
            // Half-written downloads and staging files are not kept.
            if name.ends_with(".part") || name.starts_with(".outgoing-") || name.contains(".tmp") {
                let _ = fs::remove_file(path);
                return 0;
            }
            return match self.seal_file(path) {
                Ok(()) => 0,
                Err(e) => {
                    log::warn!("could not encrypt {}: {e}", path.display());
                    1
                }
            };
        }
        if !meta.is_dir() {
            return 0;
        }
        fs::read_dir(path)
            .map(|entries| entries.flatten().map(|entry| self.seal_tree(&entry.path())).sum())
            .unwrap_or(0)
    }
}

/// Writes a sealed file as it streams in. It can be emptied and started over,
/// as a download that is retried is, but not written out of order.
pub struct Writer<W: Write + Seek> {
    sink: W,
    key: LessSafeKey,
    buf: Vec<u8>,
    index: usize,
    /// Plain bytes written.
    pos: u64,
    /// Rewound after writing: reading back is not supported, writing more is not.
    rewound: bool,
    master: [u8; 32],
}

impl Writer<File> {
    pub fn create(vault: &Vault, path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().write(true).create(true).truncate(true).open(path)?;
        set_private(path)?;
        Writer::new(vault, file)
    }
}

impl<W: Write + Seek> Writer<W> {
    fn new(vault: &Vault, sink: W) -> io::Result<Self> {
        let mut writer = Self {
            sink,
            key: vault.file_key(&[0; SALT_LEN]),
            buf: Vec::with_capacity(CHUNK + 8 * 1024),
            index: 0,
            pos: 0,
            rewound: false,
            master: vault.master,
        };
        writer.start()?;
        Ok(writer)
    }

    fn start(&mut self) -> io::Result<()> {
        let mut salt = [0u8; SALT_LEN];
        SystemRandom::new().fill(&mut salt).map_err(|_| io::Error::other("no randomness for a salt"))?;
        self.key = file_key(&self.master, &salt);
        self.sink.write_all(MAGIC)?;
        self.sink.write_all(&salt)?;
        self.buf.clear();
        self.index = 0;
        self.pos = 0;
        self.rewound = false;
        Ok(())
    }

    fn seal_front(&mut self, last: bool) -> io::Result<()> {
        let take = self.buf.len().min(CHUNK);
        let mut chunk: Vec<u8> = self.buf.drain(..take).collect();
        self.key
            .seal_in_place_append_tag(nonce(self.index, last), Aad::empty(), &mut chunk)
            .map_err(|_| io::Error::other("could not encrypt"))?;
        self.sink.write_all(&chunk)?;
        self.index += 1;
        Ok(())
    }

    /// Seals what is left as the last chunk.
    pub fn finish(mut self) -> io::Result<W> {
        self.seal_front(true)?;
        self.sink.flush()?;
        Ok(self.sink)
    }
}

impl<W: Write + Seek> Write for Writer<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.rewound {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "an encrypted file is written in one pass"));
        }
        self.buf.extend_from_slice(data);
        self.pos += data.len() as u64;
        // The last chunk is held back until `finish`, which marks it.
        while self.buf.len() > CHUNK {
            self.seal_front(false)?;
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<W: Write + Seek> Seek for Writer<W> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        match to {
            SeekFrom::Current(0) | SeekFrom::End(0) => Ok(self.pos),
            SeekFrom::Start(n) if n == self.pos => Ok(n),
            SeekFrom::Start(0) => {
                self.rewound = true;
                Ok(0)
            }
            _ => Err(io::Error::new(io::ErrorKind::Unsupported, "an encrypted file is written in one pass")),
        }
    }
}

impl whatsapp_rust::wacore::download::DownloadWriter for Writer<File> {
    fn truncate(&mut self, len: u64) -> io::Result<()> {
        if len != 0 {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "an encrypted file is only emptied"));
        }
        self.sink.set_len(0)?;
        self.sink.rewind()?;
        self.start()
    }
}

fn nonce(index: usize, last: bool) -> Nonce {
    let mut bytes = [0u8; 12];
    bytes[0] = last as u8;
    bytes[4..].copy_from_slice(&(index as u64).to_be_bytes());
    Nonce::assume_unique_for_key(bytes)
}

fn open_chunk<'a>(key: &LessSafeKey, index: usize, last: bool, sealed: &'a mut [u8]) -> io::Result<&'a mut [u8]> {
    key.open_in_place(nonce(index, last), Aad::empty(), sealed)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "the file is damaged or was not encrypted here"))
}

/// Chunks in a sealed body of `body` bytes. Even an empty file has one.
fn chunk_count(body: usize) -> io::Result<usize> {
    let chunks = body.div_ceil(SEALED_CHUNK);
    if chunks == 0 || body - (chunks - 1) * SEALED_CHUNK < TAG {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "the encrypted file is cut short"));
    }
    Ok(chunks)
}

fn sealed_len(plain: usize) -> usize {
    HEADER + plain + plain.div_ceil(CHUNK).max(1) * TAG
}

fn is_sealed(data: &[u8]) -> bool {
    data.len() >= HEADER && &data[..MAGIC.len()] == MAGIC
}

fn read_up_to(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match file.read(&mut buf[n..])? {
            0 => break,
            read => n += read,
        }
    }
    Ok(n)
}

fn temp_beside(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{}.tmp", NEXT.fetch_add(1, Ordering::Relaxed)));
    PathBuf::from(name)
}

fn set_private(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    let _ = path;
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = temp_beside(path);
    fs::write(&tmp, bytes)?;
    set_private(&tmp)?;
    fs::rename(&tmp, path)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    let text = text.trim();
    let mut out = [0u8; 32];
    if text.len() != 64 {
        return None;
    }
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// `x'…'`, the form SQLCipher takes a raw key in, skipping its slow
/// passphrase derivation.
pub fn sqlcipher_key(key: &[u8; 32]) -> String {
    format!("x'{}'", hex(key))
}

fn read_key_file(path: &Path) -> Result<[u8; 32], String> {
    let text = fs::read_to_string(path).map_err(|e| format!("could not read the storage key: {e}"))?;
    unhex(&text).ok_or_else(|| "the storage key file is damaged".into())
}

/// Where plain copies of files go while an app outside this one uses them:
/// in memory, private to the user, and emptied at every start.
pub fn runtime_dir() -> io::Result<PathBuf> {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    if let Some(dir) = DIR.get() {
        return Ok(dir.clone());
    }
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = base.join("omega-zap");
    if dir.exists() {
        let _ = fs::remove_dir_all(&dir);
    }
    crate::account::secure_dir(&dir)?;
    Ok(DIR.get_or_init(|| dir).clone())
}

/// A fresh path in the runtime directory, for a program to write a plain
/// file to that is then sealed with `import`.
pub fn handoff_path(prefix: &str) -> io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    Ok(runtime_dir()?.join(format!(".{prefix}-{}", NEXT.fetch_add(1, Ordering::Relaxed))))
}

/// Removes the plain copies made for other apps.
pub fn clear_runtime_dir() {
    if let Some(base) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).or_else(|| Some(std::env::temp_dir())) {
        let _ = fs::remove_dir_all(base.join("omega-zap"));
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod keyring {
    use std::collections::HashMap;

    use secret_service::{EncryptionType, SecretService};

    const ATTRS: [(&str, &str); 2] = [("application", crate::account::IDENTIFIER), ("purpose", "storage-key")];
    /// Where builds before the identifier changed kept it.
    const OLD_ATTRS: [(&str, &str); 2] = [("application", crate::account::OLD_IDENTIFIER), ("purpose", "storage-key")];

    /// A key in a file means the keyring was not there at the first start;
    /// asking again at every start could block on a desktop without one.
    pub const TAKES_OVER_FILE: bool = false;

    fn attrs(pairs: [(&'static str, &'static str); 2]) -> HashMap<&'static str, &'static str> {
        pairs.into_iter().collect()
    }

    pub fn load() -> Result<[u8; 32], String> {
        // The keyring can still be starting with the session; give it a moment.
        let mut last = String::new();
        for attempt in 0..5 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            match tauri::async_runtime::block_on(get(ATTRS)) {
                Ok(Some(key)) => return Ok(key),
                Ok(None) => return take_old(),
                Err(e) => last = e,
            }
        }
        Err(format!("Could not read the storage key from the keyring: {last}"))
    }

    /// Moves the key kept under the old identifier to the new one, removing
    /// the old item only once the new one reads back.
    fn take_old() -> Result<[u8; 32], String> {
        let Some(key) = tauri::async_runtime::block_on(get(OLD_ATTRS))? else {
            return Err("The storage key is missing from the keyring.".into());
        };
        if let Err(e) = save(&key) {
            log::warn!("the storage key stays under the old name: {e}");
            return Ok(key);
        }
        let removed = tauri::async_runtime::block_on(async {
            let service = SecretService::connect(EncryptionType::Dh).await.map_err(|e| e.to_string())?;
            let found = service.search_items(attrs(OLD_ATTRS)).await.map_err(|e| e.to_string())?;
            for item in found.unlocked.iter().chain(&found.locked) {
                item.delete().await.map_err(|e| e.to_string())?;
            }
            Ok::<(), String>(())
        });
        if let Err(e) = removed {
            log::warn!("could not remove the storage key under the old name: {e}");
        }
        Ok(key)
    }

    async fn get(pairs: [(&'static str, &'static str); 2]) -> Result<Option<[u8; 32]>, String> {
        let service = SecretService::connect(EncryptionType::Dh).await.map_err(|e| e.to_string())?;
        let found = service.search_items(attrs(pairs)).await.map_err(|e| e.to_string())?;
        let item = match (found.unlocked.into_iter().next(), found.locked.into_iter().next()) {
            (Some(item), _) => item,
            (None, Some(item)) => {
                item.unlock().await.map_err(|e| e.to_string())?;
                item
            }
            (None, None) => return Ok(None),
        };
        let secret = item.get_secret().await.map_err(|e| e.to_string())?;
        Ok(Some(secret.try_into().map_err(|_| "the storage key in the keyring is damaged")?))
    }

    pub fn save(key: &[u8; 32]) -> Result<(), String> {
        tauri::async_runtime::block_on(async {
            let service = SecretService::connect(EncryptionType::Dh).await.map_err(|e| e.to_string())?;
            let collection = service.get_default_collection().await.map_err(|e| e.to_string())?;
            if collection.is_locked().await.map_err(|e| e.to_string())? {
                collection.unlock().await.map_err(|e| e.to_string())?;
            }
            collection
                .create_item("Omega Zap storage key", attrs(ATTRS), key, true, "application/octet-stream")
                .await
                .map_err(|e| e.to_string())?;
            Ok::<(), String>(())
        })?;
        // Read back, so a keyring that silently drops it is not trusted.
        match tauri::async_runtime::block_on(get(ATTRS)) {
            Ok(Some(stored)) if &stored == key => Ok(()),
            _ => Err("the keyring did not keep the key".into()),
        }
    }
}

#[cfg(target_os = "macos")]
mod keyring {
    //! The login Keychain, as a generic password. The item trusts the app that
    //! made it; a build signed differently (every unsigned build) is let in by
    //! the user once, through the system's prompt.

    use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};

    use crate::account::{IDENTIFIER as SERVICE, OLD_IDENTIFIER as OLD_SERVICE};

    const ACCOUNT: &str = "storage-key";
    /// errSecItemNotFound
    const NOT_FOUND: i32 = -25300;

    /// A key left in a file by an older build moves here.
    pub const TAKES_OVER_FILE: bool = true;

    pub fn load() -> Result<[u8; 32], String> {
        match read(SERVICE)? {
            Some(key) => Ok(key),
            None => take_old(),
        }
    }

    fn read(service: &str) -> Result<Option<[u8; 32]>, String> {
        match get_generic_password(service, ACCOUNT) {
            Ok(secret) => Ok(Some(secret.try_into().map_err(|_| "the storage key in the Keychain is damaged")?)),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(format!("Could not read the storage key from the Keychain: {e}")),
        }
    }

    /// Moves the key kept under the old identifier to the new one, removing
    /// the old item only once the new one reads back.
    fn take_old() -> Result<[u8; 32], String> {
        let Some(key) = read(OLD_SERVICE)? else {
            return Err("The storage key is missing from the Keychain.".into());
        };
        if let Err(e) = save(&key) {
            log::warn!("the storage key stays under the old name: {e}");
            return Ok(key);
        }
        if let Err(e) = delete_generic_password(OLD_SERVICE, ACCOUNT) {
            log::warn!("could not remove the storage key under the old name: {e}");
        }
        Ok(key)
    }

    pub fn save(key: &[u8; 32]) -> Result<(), String> {
        set_generic_password(SERVICE, ACCOUNT, key).map_err(|e| format!("the Keychain refused the key: {e}"))?;
        // Read back, so a Keychain that silently drops it is not trusted.
        match read(SERVICE) {
            Ok(Some(stored)) if &stored == key => Ok(()),
            _ => Err("the Keychain did not keep the key".into()),
        }
    }
}

#[cfg(not(unix))]
mod keyring {
    pub const TAKES_OVER_FILE: bool = false;

    pub fn load() -> Result<[u8; 32], String> {
        Err("There is no keyring support on this system.".into())
    }

    pub fn save(_: &[u8; 32]) -> Result<(), String> {
        Err("no keyring support on this system".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> Vault {
        Vault { master: [7; 32] }
    }

    #[test]
    fn round_trips_at_chunk_edges() {
        let v = vault();
        for len in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 3 * CHUNK, 3 * CHUNK + 5] {
            let plain: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let sealed = v.seal_bytes(&plain);
            assert_eq!(sealed.len(), sealed_len(len), "{len}");
            assert_eq!(v.open_bytes(&sealed).unwrap(), plain, "{len}");
        }
    }

    #[test]
    fn tampering_and_truncation_are_caught() {
        let v = vault();
        let plain = vec![1u8; 2 * CHUNK + 10];
        let mut sealed = v.seal_bytes(&plain);
        // Cut at a chunk boundary: the new last chunk was not sealed as last.
        assert!(v.open_bytes(&sealed[..HEADER + 2 * SEALED_CHUNK]).is_err());
        sealed[HEADER + 5] ^= 1;
        assert!(v.open_bytes(&sealed).is_err());
        assert!(Vault { master: [8; 32] }.open_bytes(&v.seal_bytes(b"x")).is_err());
    }

    #[test]
    fn plain_files_from_before_still_read() {
        assert_eq!(vault().open_bytes(b"{\"chats\":{}}").unwrap(), b"{\"chats\":{}}");
    }

    #[test]
    fn ranges_decrypt_only_what_they_cover() {
        let v = vault();
        let dir = std::env::temp_dir().join(format!("oz-vault-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f");
        let plain: Vec<u8> = (0..3 * CHUNK + 100).map(|i| (i % 253) as u8).collect();
        fs::write(&path, v.seal_bytes(&plain)).unwrap();
        assert_eq!(v.len(&path).unwrap(), plain.len() as u64);
        for (s, e) in [(0, 0), (0, 9), (CHUNK - 3, CHUNK + 3), (10, 3 * CHUNK + 99), (3 * CHUNK + 99, 3 * CHUNK + 99)] {
            assert_eq!(v.read_range(&path, s as u64, e as u64).unwrap(), &plain[s..=e], "{s}-{e}");
        }
        fs::write(&path, &plain).unwrap();
        v.seal_file(&path).unwrap();
        assert_eq!(v.read(&path).unwrap(), plain);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_retried_download_starts_over() {
        use whatsapp_rust::wacore::download::DownloadWriter;
        let v = vault();
        let dir = std::env::temp_dir().join(format!("oz-vault-w-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f");
        let mut w = Writer::create(&v, &path).unwrap();
        w.write_all(&vec![9u8; CHUNK * 2]).unwrap();
        w.truncate(0).unwrap();
        w.rewind().unwrap();
        w.write_all(b"second try").unwrap();
        w.rewind().unwrap();
        assert!(w.write_all(b"more").is_err());
        w.finish().unwrap();
        assert_eq!(v.read(&path).unwrap(), b"second try");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn derived_keys_differ_by_purpose() {
        let v = vault();
        assert_ne!(v.derive("a"), v.derive("b"));
        assert_eq!(unhex(&hex(&v.derive("a"))), Some(v.derive("a")));
    }
}

