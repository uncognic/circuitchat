pub const MSG_VERSION_NEGOTIATE: u8 = 0xFF;

pub fn protocol_version() -> (u8, u8, u8) {
    let v = env!("CARGO_PKG_VERSION");
    let mut parts = v.split(|c| c == '.' || c == '-');
    let major = parts.next().and_then(|s| s.parse::<u8>().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse::<u8>().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse::<u8>().ok()).unwrap_or(0);
    (major, minor, patch)
}

pub fn encode_version_negotiate() -> Vec<u8> {
    let (major, minor, patch) = protocol_version();
    vec![0x00, MSG_VERSION_NEGOTIATE, major, minor, patch]
}
use rand::Rng;
use rand::distributions::Alphanumeric;
use std::error::Error;
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use xxhash_rust::xxh3::Xxh3;

const CHUNK_SIZE: usize = 60_000;
const OFFER_TAG: u8 = b'F';
const CHUNK_TAG: u8 = b'C';
const DONE_TAG: u8 = b'D';
const CANCEL_TAG: u8 = b'X';
pub const MSG_FILE_ACCEPT: u8 = 0x05;
pub const MSG_FILE_REJECT: u8 = 0x06;
pub const MSG_TYPING_START: u8 = 0x07;
pub const MSG_TYPING_STOP: u8 = 0x08;
pub const MSG_DELIVERED: u8 = 0x09;
pub const MSG_PING: u8 = 0x0A;
pub const MSG_PONG: u8 = 0x0B;

pub fn encode_typing_start() -> Vec<u8> {
    vec![0x00, MSG_TYPING_START]
}
pub fn encode_typing_stop() -> Vec<u8> {
    vec![0x00, MSG_TYPING_STOP]
}
pub fn encode_delivered() -> Vec<u8> {
    vec![0x00, MSG_DELIVERED]
}
pub fn encode_ping() -> Vec<u8> {
    vec![0x00, MSG_PING]
}
pub fn encode_pong() -> Vec<u8> {
    vec![0x00, MSG_PONG]
}
//pub fn encode_accept() -> Vec<u8> {
//    vec![0x00, MSG_FILE_ACCEPT]
//}

pub fn encode_accept_with_offset(offset: u64) -> Vec<u8> {
    let mut msg = vec![0x00, MSG_FILE_ACCEPT];
    msg.extend_from_slice(&offset.to_be_bytes());
    msg
}
pub fn encode_reject() -> Vec<u8> {
    vec![0x00, MSG_FILE_REJECT]
}
/*
pub fn encode_offer(name: &str, size: u64) -> Vec<u8> {
    let mut msg = vec![0x00, OFFER_TAG];
    msg.extend_from_slice(&size.to_be_bytes());
    msg.extend_from_slice(name.as_bytes());
    msg
}
*/
pub fn encode_offer_with_checksum(name: &str, size: u64, checksum: Option<&[u8]>) -> Vec<u8> {
    let mut msg = vec![0x00, OFFER_TAG];
    msg.extend_from_slice(&size.to_be_bytes());
    if let Some(c) = checksum {
        msg.extend_from_slice(c);
    }
    msg.extend_from_slice(name.as_bytes());
    msg
}

pub fn encode_chunk(data: &[u8]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(2 + data.len());
    msg.push(0x00);
    msg.push(CHUNK_TAG);
    msg.extend_from_slice(data);
    msg
}

pub fn encode_done() -> Vec<u8> {
    vec![0x00, DONE_TAG]
}

pub fn encode_cancel() -> Vec<u8> {
    vec![0x00, CANCEL_TAG]
}

pub enum ParsedMessage {
    Text(String),
    FileOffer {
        name: String,
        size: u64,
        checksum: Option<Vec<u8>>,
    },
    FileAccept(u64),
    FileReject,
    FileChunk(Vec<u8>),
    FileDone,
    FileCancel,
    TypingStart,
    TypingStop,
    Delivered,
    Ping,
    Pong,
    VersionNegotiate {
        major: u8,
        minor: u8,
        patch: u8,
    },
}

pub fn parse_message(data: &[u8]) -> ParsedMessage {
    if data.len() >= 2 && data[0] == 0x00 {
        match data[1] {
            MSG_VERSION_NEGOTIATE if data.len() >= 5 => {
                let major = data[2];
                let minor = data[3];
                let patch = data[4];
                ParsedMessage::VersionNegotiate {
                    major,
                    minor,
                    patch,
                }
            }
            OFFER_TAG if data.len() >= 10 => {
                let size = u64::from_be_bytes(data[2..10].try_into().unwrap());
                if data.len() >= 18 {
                    let checksum = data[10..18].to_vec();
                    let name = String::from_utf8_lossy(&data[18..]).to_string();
                    ParsedMessage::FileOffer {
                        name,
                        size,
                        checksum: Some(checksum),
                    }
                } else {
                    let name = String::from_utf8_lossy(&data[10..]).to_string();
                    ParsedMessage::FileOffer {
                        name,
                        size,
                        checksum: None,
                    }
                }
            }
            CHUNK_TAG => ParsedMessage::FileChunk(data[2..].to_vec()),
            DONE_TAG => ParsedMessage::FileDone,
            CANCEL_TAG => ParsedMessage::FileCancel,
            MSG_FILE_ACCEPT => {
                if data.len() >= 10 {
                    let offset = u64::from_be_bytes(data[2..10].try_into().unwrap());
                    ParsedMessage::FileAccept(offset)
                } else {
                    ParsedMessage::FileAccept(0)
                }
            }
            MSG_FILE_REJECT => ParsedMessage::FileReject,
            MSG_TYPING_START => ParsedMessage::TypingStart,
            MSG_TYPING_STOP => ParsedMessage::TypingStop,
            MSG_DELIVERED => ParsedMessage::Delivered,
            MSG_PING => ParsedMessage::Ping,
            MSG_PONG => ParsedMessage::Pong,
            _ => ParsedMessage::Text(String::from_utf8_lossy(data).to_string()),
        }
    } else {
        ParsedMessage::Text(String::from_utf8_lossy(data).to_string())
    }
}

pub struct IncomingFile {
    pub name: String,
    pub size: u64,
    pub received: u64,
    writer: std::io::BufWriter<fs::File>,
    path: PathBuf,
}

impl IncomingFile {
    pub fn begin(
        name: &str,
        size: u64,
        expected_checksum: Option<&[u8]>,
    ) -> Result<Self, Box<dyn Error>> {
        let dir = downloads_dir()?;
        fs::create_dir_all(&dir)?;

        let sanitized = sanitize_filename(name);
        let path = dir.join(&sanitized);

        let file_exists = path.exists();
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let received = if file_exists {
            file.metadata()?.len()
        } else {
            0
        };
        let writer = std::io::BufWriter::new(file);

        if let Some(sum) = expected_checksum {
            let meta_path = path.with_file_name(format!(
                "{}.xxh3",
                path.file_name().unwrap().to_string_lossy()
            ));
            let mut mf = fs::File::create(&meta_path)?;
            let hexstr = hex::encode(sum);
            mf.write_all(hexstr.as_bytes())?;
        }

        Ok(IncomingFile {
            name: sanitized,
            size,
            received,
            writer,
            path,
        })
    }

    pub fn write_chunk(&mut self, data: &[u8]) -> Result<(), Box<dyn Error>> {
        self.writer.write_all(data)?;
        self.received += data.len() as u64;
        Ok(())
    }

    pub fn finish(mut self) -> Result<PathBuf, Box<dyn Error>> {
        self.writer.flush()?;

        let meta_path = self.path.with_file_name(format!(
            "{}.xxh3",
            self.path.file_name().unwrap().to_string_lossy()
        ));
        if meta_path.exists() {
            if let Ok(expected_hex) = fs::read_to_string(&meta_path) {
                if !expected_hex.trim().is_empty() {
                    let expected = hex::decode(expected_hex.trim())?;
                    let actual = file_xxh3(&self.path)?;
                    if expected != actual {
                        return Err(From::from("checksum mismatch after download"));
                    }
                }
            }
            let _ = fs::remove_file(&meta_path);
        }
        Ok(self.path)
    }

    pub fn cancel(self) {
        drop(self.writer);
        let _ = fs::remove_file(&self.path);
    }
}

pub struct OutgoingFile {
    pub name: String,
    pub size: u64,
    pub sent: u64,
    pub checksum: Vec<u8>,
    reader: std::io::BufReader<fs::File>,
}

impl OutgoingFile {
    pub fn open(path: &str) -> Result<Self, Box<dyn Error>> {
        let path = path.trim();
        let metadata = fs::metadata(path)?;
        let size = metadata.len();
        let name = Path::new(path)
            .file_name()
            .ok_or("invalid file path")?
            .to_string_lossy()
            .to_string();

        let mut hasher = Xxh3::new();
        let mut hfile = fs::File::open(path)?;
        let mut hreader = std::io::BufReader::new(&mut hfile);
        let mut buf = [0u8; 8192];
        loop {
            let n = hreader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        let checksum_u = hasher.digest();
        let checksum = checksum_u.to_be_bytes().to_vec();

        let file = fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        Ok(OutgoingFile {
            name,
            size,
            sent: 0,
            checksum,
            reader,
        })
    }

    pub fn read_next_chunk(&mut self) -> Result<Option<Vec<u8>>, Box<dyn Error>> {
        let mut buf = vec![0u8; CHUNK_SIZE];
        let n = self.reader.read(&mut buf)?;
        if n == 0 {
            return Ok(None);
        }
        buf.truncate(n);
        self.sent += n as u64;
        Ok(Some(buf))
    }

    pub fn seek_to(&mut self, offset: u64) -> Result<(), Box<dyn Error>> {
        use std::io::SeekFrom;
        self.reader.get_mut().seek(SeekFrom::Start(offset))?;
        self.sent = offset;
        Ok(())
    }
}
fn downloads_dir() -> Result<PathBuf, Box<dyn Error>> {
    let exe_dir = std::env::current_exe()?
        .parent()
        .ok_or("could not determine executable directory")?
        .to_path_buf();
    Ok(exe_dir.join("downloads"))
}

pub fn remove_downloads_dir() -> Result<(), Box<dyn Error>> {
    let dir = downloads_dir()?;
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    Ok(())
}

pub fn download_path(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = downloads_dir()?;
    Ok(dir.join(sanitize_filename(name)))
}

pub fn file_xxh3(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    use xxhash_rust::xxh3::Xxh3;
    let mut hasher = Xxh3::new();
    let file = fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.digest().to_be_bytes().to_vec())
}

pub fn sanitize_filename(name: &str) -> String {
    let name = Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unnamed".to_string());

    name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_")
}

pub fn existing_download_size(name: &str) -> Result<u64, Box<dyn Error>> {
    let dir = downloads_dir()?;
    let sanitized = sanitize_filename(name);
    let path = dir.join(&sanitized);
    if path.exists() {
        Ok(fs::metadata(path)?.len())
    } else {
        Ok(0)
    }
}
/*
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let base = dir.join(name);
    if !base.exists() {
        return base;
    }

    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    let ext = Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();

    for i in 1u32.. {
        let candidate = dir.join(format!("{} ({}){}", stem, i, ext));
        if !candidate.exists() {
            return candidate;
        }
    }

    base
}
*/
pub fn randomize_filename_preserve_ext(name: &str) -> String {
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_string());

    let token: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(12)
        .map(char::from)
        .collect();

    match ext {
        Some(e) if !e.is_empty() => format!("{}.{}", token, e),
        _ => token,
    }
}

pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
