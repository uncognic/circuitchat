use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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

pub fn encode_typing_start() -> Vec<u8> {
    vec![0x00, MSG_TYPING_START]
}
pub fn encode_typing_stop() -> Vec<u8> {
    vec![0x00, MSG_TYPING_STOP]
}
pub fn encode_delivered() -> Vec<u8> {
    vec![0x00, MSG_DELIVERED]
}
pub fn encode_accept() -> Vec<u8> {
    vec![0x00, MSG_FILE_ACCEPT]
}
pub fn encode_reject() -> Vec<u8> {
    vec![0x00, MSG_FILE_REJECT]
}

pub fn encode_offer(name: &str, size: u64) -> Vec<u8> {
    let mut msg = vec![0x00, OFFER_TAG];
    msg.extend_from_slice(&size.to_be_bytes());
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
    FileOffer { name: String, size: u64 },
    FileAccept,
    FileReject,
    FileChunk(Vec<u8>),
    FileDone,
    FileCancel,
    TypingStart,
    TypingStop,
    Delivered,
}

pub fn parse_message(data: &[u8]) -> ParsedMessage {
    if data.len() >= 2 && data[0] == 0x00 {
        match data[1] {
            OFFER_TAG if data.len() >= 10 => {
                let size = u64::from_be_bytes(data[2..10].try_into().unwrap());
                let name = String::from_utf8_lossy(&data[10..]).to_string();
                ParsedMessage::FileOffer { name, size }
            }
            CHUNK_TAG => ParsedMessage::FileChunk(data[2..].to_vec()),
            DONE_TAG => ParsedMessage::FileDone,
            CANCEL_TAG => ParsedMessage::FileCancel,
            MSG_FILE_ACCEPT => ParsedMessage::FileAccept,
            MSG_FILE_REJECT => ParsedMessage::FileReject,
            MSG_TYPING_START => ParsedMessage::TypingStart,
            MSG_TYPING_STOP => ParsedMessage::TypingStop,
            MSG_DELIVERED => ParsedMessage::Delivered,
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
    pub fn begin(name: &str, size: u64) -> Result<Self, Box<dyn Error>> {
        let dir = downloads_dir()?;
        fs::create_dir_all(&dir)?;

        let sanitized = sanitize_filename(name);
        let path = unique_path(&dir, &sanitized);

        let file = fs::File::create(&path)?;
        let writer = std::io::BufWriter::new(file);

        Ok(IncomingFile {
            name: sanitized,
            size,
            received: 0,
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
        let file = fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        Ok(OutgoingFile {
            name,
            size,
            sent: 0,
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
}
fn downloads_dir() -> Result<PathBuf, Box<dyn Error>> {
    let exe_dir = std::env::current_exe()?
        .parent()
        .ok_or("could not determine executable directory")?
        .to_path_buf();
    Ok(exe_dir.join("downloads"))
}

fn sanitize_filename(name: &str) -> String {
    let name = Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unnamed".to_string());

    name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_")
}

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
