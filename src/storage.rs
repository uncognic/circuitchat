use argon2::Argon2;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{AeadCore, XChaCha20Poly1305, XNonce};
use rand::RngCore;
use rusqlite::Connection;
use std::error::Error;
use std::path::PathBuf;

fn derive_key(passphrase: &str, salt: &[u8; 16]) -> Result<[u8; 32], Box<dyn Error>> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| format!("key derivation failed: {}", e))?;
    Ok(key)
}

fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| format!("encryption failed: {}", e))?;

    let mut out = Vec::with_capacity(nonce.len() + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

fn decrypt(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    if data.len() < 24 {
        return Err("ciphertext too short".into());
    }
    let (nonce_bytes, ct) = data.split_at(24);
    let nonce = XNonce::from_slice(nonce_bytes);
    let cipher = XChaCha20Poly1305::new(key.into());
    let plaintext = cipher
        .decrypt(nonce, ct)
        .map_err(|_| "decryption failed, wrong passphrase?")?;
    Ok(plaintext)
}

pub struct Storage {
    conn: Connection,
    key: [u8; 32],
}

impl Storage {
    pub fn open(passphrase: &str) -> Result<Self, Box<dyn Error>> {
        let db_path = db_path()?;
        let is_new = !db_path.exists();
        let conn = Connection::open(&db_path)?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (
                 id    INTEGER PRIMARY KEY CHECK (id = 1),
                 salt  BLOB NOT NULL,
                 check_blob BLOB NOT NULL
             );

             CREATE TABLE IF NOT EXISTS messages (
                 id        INTEGER PRIMARY KEY AUTOINCREMENT,
                 direction TEXT NOT NULL CHECK (direction IN ('sent', 'received')),
                 content   BLOB NOT NULL,
                 timestamp INTEGER NOT NULL
             );",
        )?;

        let key = if is_new {
            let mut salt = [0u8; 16];
            rand::thread_rng().fill_bytes(&mut salt);
            let key = derive_key(passphrase, &salt)?;

            let check = encrypt(&key, b"circuitchat")?;
            conn.execute(
                "INSERT INTO meta (id, salt, check_blob) VALUES (1, ?1, ?2)",
                rusqlite::params![salt.as_slice(), check],
            )?;
            key
        } else {
            let (salt_vec, check_blob): (Vec<u8>, Vec<u8>) = conn.query_row(
                "SELECT salt, check_blob FROM meta WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;

            let salt: [u8; 16] = salt_vec
                .try_into()
                .map_err(|_| "corrupt salt in database")?;
            let key = derive_key(passphrase, &salt)?;

            decrypt(&key, &check_blob)?;
            key
        };

        Ok(Storage { conn, key })
    }

    pub fn save_message(
        &self,
        direction: MessageDirection,
        content: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        let encrypted = encrypt(&self.key, content)?;

        self.conn.execute(
            "INSERT INTO messages (direction, content, timestamp) VALUES (?1, ?2, ?3)",
            rusqlite::params![direction.as_str(), encrypted, timestamp],
        )?;

        Ok(())
    }

    pub fn load_history(&self) -> Result<Vec<Message>, Box<dyn Error>> {
        let mut stmt = self
            .conn
            .prepare("SELECT direction, content, timestamp FROM messages ORDER BY timestamp ASC")?;

        let rows: Vec<(String, Vec<u8>, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut messages = Vec::with_capacity(rows.len());
        for (direction, encrypted, timestamp) in rows {
            let content = decrypt(&self.key, &encrypted)?;
            messages.push(Message {
                direction: MessageDirection::from_str(&direction),
                content,
                timestamp,
            });
        }

        Ok(messages)
    }
}

pub fn db_path() -> Result<PathBuf, Box<dyn Error>> {
    let exe_dir = std::env::current_exe()?
        .parent()
        .ok_or("could not determine exe directory")?
        .to_path_buf();
    Ok(exe_dir.join("circuitchat.db"))
}

pub struct Message {
    pub direction: MessageDirection,
    pub content: Vec<u8>,
    pub timestamp: i64,
}

pub enum MessageDirection {
    Sent,
    Received,
}

impl MessageDirection {
    fn as_str(&self) -> &'static str {
        match self {
            MessageDirection::Sent => "sent",
            MessageDirection::Received => "received",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "sent" => MessageDirection::Sent,
            _ => MessageDirection::Received,
        }
    }
}
