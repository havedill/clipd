use crate::config::Config;
use crate::crypto::Crypto;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    Image,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Image => "image",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "text" => Some(Kind::Text),
            "image" => Some(Kind::Image),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemMeta {
    pub id: i64,
    pub kind: Kind,
    pub mime: String,
    pub created_at: i64,
    pub last_used_at: i64,
    pub size: i64,
    /// Plaintext preview for text, or empty for images.
    pub preview: String,
    /// Decrypted thumbnail PNG bytes (images only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb: Option<Vec<u8>>,
}

pub struct Store {
    conn: Connection,
    crypto: Crypto,
    max_items: usize,
}

impl Store {
    pub fn set_max_items(&mut self, max_items: usize) {
        self.max_items = max_items.max(1);
        let _ = self.evict();
    }

    pub fn open(cfg: &Config) -> Result<Self> {
        let dir = Config::data_dir();
        std::fs::create_dir_all(&dir)?;
        let db = Config::db_path();
        Self::open_path(&db, cfg.max_items)
    }

    pub fn open_path(path: &Path, max_items: usize) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS items (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               kind TEXT NOT NULL,
               mime TEXT NOT NULL,
               created_at INTEGER NOT NULL,
               last_used_at INTEGER NOT NULL,
               size INTEGER NOT NULL,
               content_hash TEXT NOT NULL,
               ciphertext BLOB NOT NULL,
               nonce BLOB NOT NULL,
               thumb_ciphertext BLOB,
               thumb_nonce BLOB,
               preview_cipher BLOB,
               preview_nonce BLOB
             );
             CREATE INDEX IF NOT EXISTS idx_items_last_used ON items(last_used_at DESC);",
        )?;
        Ok(Self {
            conn,
            crypto: Crypto::open()?,
            max_items,
        })
    }

    pub fn count(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    pub fn insert(&mut self, kind: Kind, mime: &str, payload: &[u8]) -> Result<Option<i64>> {
        let hash = hex_hash(payload);
        // Consecutive dedupe: skip if newest item has same hash.
        let newest: Option<String> = self
            .conn
            .query_row(
                "SELECT content_hash FROM items ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if newest.as_deref() == Some(hash.as_str()) {
            return Ok(None);
        }

        let now = now_secs();
        let (ciphertext, nonce) = self.crypto.encrypt(payload)?;

        let (thumb_ct, thumb_n, preview_ct, preview_n) = match kind {
            Kind::Text => {
                let preview = preview_text(payload);
                let (c, n) = self.crypto.encrypt(preview.as_bytes())?;
                (None, None, Some(c), Some(n))
            }
            Kind::Image => {
                let thumb = make_thumb(payload).unwrap_or_default();
                if thumb.is_empty() {
                    (None, None, None, None)
                } else {
                    let (c, n) = self.crypto.encrypt(&thumb)?;
                    (Some(c), Some(n), None, None)
                }
            }
        };

        self.conn.execute(
            "INSERT INTO items (kind, mime, created_at, last_used_at, size, content_hash,
             ciphertext, nonce, thumb_ciphertext, thumb_nonce, preview_cipher, preview_nonce)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                kind.as_str(),
                mime,
                now,
                now,
                payload.len() as i64,
                hash,
                ciphertext,
                nonce,
                thumb_ct,
                thumb_n,
                preview_ct,
                preview_n,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.evict()?;
        Ok(Some(id))
    }

    fn evict(&self) -> Result<()> {
        let count = self.count()?;
        if count <= self.max_items {
            return Ok(());
        }
        let drop_n = (count - self.max_items) as i64;
        self.conn.execute(
            "DELETE FROM items WHERE id IN (
               SELECT id FROM items ORDER BY last_used_at ASC, id ASC LIMIT ?1
             )",
            params![drop_n],
        )?;
        Ok(())
    }

    pub fn list(&self, limit: usize) -> Result<Vec<ItemMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, mime, created_at, last_used_at, size,
                    thumb_ciphertext, thumb_nonce, preview_cipher, preview_nonce
             FROM items ORDER BY last_used_at DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<Vec<u8>>>(6)?,
                row.get::<_, Option<Vec<u8>>>(7)?,
                row.get::<_, Option<Vec<u8>>>(8)?,
                row.get::<_, Option<Vec<u8>>>(9)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (id, kind_s, mime, created_at, last_used_at, size, thumb_ct, thumb_n, prev_ct, prev_n) =
                row?;
            let kind = Kind::parse(&kind_s).unwrap_or(Kind::Text);
            let preview = match (prev_ct, prev_n) {
                (Some(c), Some(n)) => String::from_utf8_lossy(&self.crypto.decrypt(&c, &n)?).into_owned(),
                _ => String::new(),
            };
            let thumb = match (thumb_ct, thumb_n) {
                (Some(c), Some(n)) => Some(self.crypto.decrypt(&c, &n)?),
                _ => None,
            };
            out.push(ItemMeta {
                id,
                kind,
                mime,
                created_at,
                last_used_at,
                size,
                preview,
                thumb,
            });
        }
        Ok(out)
    }

    pub fn get_payload(&self, id: i64) -> Result<(Kind, String, Vec<u8>)> {
        let (kind_s, mime, ct, nonce): (String, String, Vec<u8>, Vec<u8>) = self.conn.query_row(
            "SELECT kind, mime, ciphertext, nonce FROM items WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let kind = Kind::parse(&kind_s).context("bad kind")?;
        let payload = self.crypto.decrypt(&ct, &nonce)?;
        Ok((kind, mime, payload))
    }

    pub fn touch(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE items SET last_used_at=?1 WHERE id=?2",
            params![now_secs(), id],
        )?;
        Ok(())
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn hex_hash(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

fn preview_text(payload: &[u8]) -> String {
    let s = String::from_utf8_lossy(payload);
    let one_line: String = s.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    let trimmed = one_line.trim();
    if trimmed.chars().count() > 120 {
        format!("{}…", trimmed.chars().take(120).collect::<String>())
    } else {
        trimmed.to_string()
    }
}

fn make_thumb(payload: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory(payload).context("decode image")?;
    let thumb = img.thumbnail(96, 96);
    let mut buf = Vec::new();
    thumb.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Best-effort: needs Secret Service / KWallet for the key.
    #[test]
    fn insert_dedupe_and_evict() {
        let dir = std::env::temp_dir().join(format!("clipd-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("t.db");
        let Ok(mut store) = Store::open_path(&db, 3) else {
            eprintln!("skip: keyring unavailable");
            return;
        };
        assert!(store.insert(Kind::Text, "text/plain", b"a").unwrap().is_some());
        assert!(store.insert(Kind::Text, "text/plain", b"a").unwrap().is_none());
        assert!(store.insert(Kind::Text, "text/plain", b"b").unwrap().is_some());
        assert!(store.insert(Kind::Text, "text/plain", b"c").unwrap().is_some());
        assert!(store.insert(Kind::Text, "text/plain", b"d").unwrap().is_some());
        assert_eq!(store.count().unwrap(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
