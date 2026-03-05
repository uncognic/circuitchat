use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Connect,
    Disconnect,
    Message,
    File,
}

#[derive(Debug, Clone)]
pub enum Condition {
    Contains(String),
    StartsWith(String),
    EndsWith(String),
    Equals(String),
    Not(Box<Condition>),
    FileSizeGt(u64),
    FileSizeLt(u64),
    FileNameEndsWith(String),
    FileNameStartsWith(String),
    FileNameContains(String),
    FileNameEquals(String),
    MessageLengthGt(usize),
    MessageLengthLt(usize),
    MessageLengthEq(usize),
}

#[derive(Debug, Clone)]
pub enum Action {
    Reply(String),
    Log(String),
    SendFile(String),
    Accept,
    Reject,
    Disconnect,
    Wait(u64),
}

#[derive(Debug, Clone)]
pub enum Block {
    Conditional {
        condition: Condition,
        actions: Vec<Action>,
    },
    Unconditional(Action),
}

#[derive(Debug, Clone)]
pub struct Handler {
    pub event: Event,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone)]
pub struct Script {
    pub handlers: Vec<Handler>,
}

#[derive(Debug)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error line {}: {}", self.line, self.message)
    }
}

impl Error for ParseError {}


pub fn parse(source: &str) -> Result<Script, ParseError> {
    let lines: Vec<(usize, &str)> = source
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty() && !l.starts_with("//"))
        .collect();

    let mut handlers = Vec::new();
    let mut pos = 0;

    while pos < lines.len() {
        let (line_no, line) = lines[pos];

        if line.starts_with("on ") {
            let event_str = line[3..].trim();
            let event = parse_event(event_str, line_no)?;
            pos += 1;

            let mut blocks = Vec::new();

            loop {
                if pos >= lines.len() {
                    return Err(ParseError {
                        line: line_no,
                        message: format!("unterminated handler for '{}'", event_str),
                    });
                }

                let (ln, content) = lines[pos];

                if content == "end" {
                    pos += 1;
                    break;
                }

                if content.starts_with("if ") {
                    let cond = parse_condition(&content[3..], ln)?;
                    pos += 1;

                    let mut actions = Vec::new();
                    loop {
                        if pos >= lines.len() {
                            return Err(ParseError {
                                line: ln,
                                message: "unterminated if block".to_string(),
                            });
                        }
                        let (aln, acontent) = lines[pos];
                        if acontent == "end" {
                            pos += 1;
                            break;
                        }
                        actions.push(parse_action(acontent, aln)?);
                        pos += 1;
                    }

                    blocks.push(Block::Conditional {
                        condition: cond,
                        actions,
                    });
                } else {
                    let action = parse_action(content, ln)?;
                    blocks.push(Block::Unconditional(action));
                    pos += 1;
                }
            }

            handlers.push(Handler { event, blocks });
        } else {
            return Err(ParseError {
                line: line_no,
                message: format!("expected 'on <event>', found '{}'", line),
            });
        }
    }

    Ok(Script { handlers })
}

fn parse_event(s: &str, line: usize) -> Result<Event, ParseError> {
    match s {
        "connect" => Ok(Event::Connect),
        "disconnect" => Ok(Event::Disconnect),
        "message" => Ok(Event::Message),
        "file" => Ok(Event::File),
        other => Err(ParseError {
            line,
            message: format!("unknown event '{}'", other),
        }),
    }
}

fn parse_condition(s: &str, line: usize) -> Result<Condition, ParseError> {
    let s = s.trim();

    if let Some(rest) = s.strip_prefix("not ") {
        let inner = parse_condition(rest.trim(), line)?;
        return Ok(Condition::Not(Box::new(inner)));
    }

    if let Some(rest) = s.strip_prefix("message_length") {
        let rest = rest.trim();
        if let Some(val) = rest.strip_prefix("> ").or_else(|| rest.strip_prefix(">")) {
            let n: usize = val.trim().parse().map_err(|_| ParseError {
                line,
                message: format!("invalid message_length value '{}'", val.trim()),
            })?;
            return Ok(Condition::MessageLengthGt(n));
        }
        if let Some(val) = rest.strip_prefix("< ").or_else(|| rest.strip_prefix("<")) {
            let n: usize = val.trim().parse().map_err(|_| ParseError {
                line,
                message: format!("invalid message_length value '{}'", val.trim()),
            })?;
            return Ok(Condition::MessageLengthLt(n));
        }
        if let Some(val) = rest.strip_prefix("== ").or_else(|| rest.strip_prefix("==")) {
            let n: usize = val.trim().parse().map_err(|_| ParseError {
                line,
                message: format!("invalid message_length value '{}'", val.trim()),
            })?;
            return Ok(Condition::MessageLengthEq(n));
        }
        return Err(ParseError {
            line,
            message: "message_length requires >, < or == operator".to_string(),
        });
    }

    if let Some(rest) = s.strip_prefix("file_size") {
        let rest = rest.trim();
        if let Some(val) = rest.strip_prefix("> ").or_else(|| rest.strip_prefix(">")) {
            let n: u64 = val.trim().parse().map_err(|_| ParseError {
                line,
                message: format!("invalid file_size value '{}'", val.trim()),
            })?;
            return Ok(Condition::FileSizeGt(n));
        }
        if let Some(val) = rest.strip_prefix("< ").or_else(|| rest.strip_prefix("<")) {
            let n: u64 = val.trim().parse().map_err(|_| ParseError {
                line,
                message: format!("invalid file_size value '{}'", val.trim()),
            })?;
            return Ok(Condition::FileSizeLt(n));
        }
        return Err(ParseError {
            line,
            message: "file_size requires > or < operator".to_string(),
        });
    }

    if let Some(rest) = s.strip_prefix("file_name ") {
        let rest = rest.trim();
        if let Some(text) = strip_condition_with_string("ends_with", rest) {
            return Ok(Condition::FileNameEndsWith(text));
        }
        if let Some(text) = strip_condition_with_string("starts_with", rest) {
            return Ok(Condition::FileNameStartsWith(text));
        }
        if let Some(text) = strip_condition_with_string("contains", rest) {
            return Ok(Condition::FileNameContains(text));
        }
        if let Some(text) = strip_condition_with_string("equals", rest) {
            return Ok(Condition::FileNameEquals(text));
        }
        return Err(ParseError {
            line,
            message: format!("unknown file_name condition: '{}'", rest),
        });
    }

    if let Some(text) = strip_condition_with_string("contains", s) {
        return Ok(Condition::Contains(text));
    }
    if let Some(text) = strip_condition_with_string("starts_with", s) {
        return Ok(Condition::StartsWith(text));
    }
    if let Some(text) = strip_condition_with_string("ends_with", s) {
        return Ok(Condition::EndsWith(text));
    }
    if let Some(text) = strip_condition_with_string("equals", s) {
        return Ok(Condition::Equals(text));
    }

    Err(ParseError {
        line,
        message: format!("unknown condition '{}'", s),
    })
}

fn strip_condition_with_string(keyword: &str, s: &str) -> Option<String> {
    let rest = s.strip_prefix(keyword)?.trim_start();
    extract_quoted_string(rest)
}

fn extract_quoted_string(s: &str) -> Option<String> {
    let s = s.trim();
    if s.starts_with('"') && s.len() >= 2 {
        if let Some(end) = s[1..].find('"') {
            return Some(s[1..1 + end].to_string());
        }
    }
    None
}

fn parse_action(s: &str, line: usize) -> Result<Action, ParseError> {
    if let Some(rest) = s.strip_prefix("reply ") {
        let text = extract_quoted_string(rest.trim()).ok_or_else(|| ParseError {
            line,
            message: "unterminated string".to_string(),
        })?;
        return Ok(Action::Reply(text));
    }
    if let Some(rest) = s.strip_prefix("log ") {
        let text = extract_quoted_string(rest.trim()).ok_or_else(|| ParseError {
            line,
            message: "unterminated string".to_string(),
        })?;
        return Ok(Action::Log(text));
    }
    if let Some(rest) = s.strip_prefix("wait ") {
        let ms: u64 = rest.trim().parse().map_err(|_| ParseError {
            line,
            message: format!("invalid wait duration '{}'", rest.trim()),
        })?;
        return Ok(Action::Wait(ms));
    }
    if let Some(rest) = s.strip_prefix("send_file ") {
        let text = extract_quoted_string(rest.trim()).ok_or_else(|| ParseError {
            line,
            message: "unterminated string".to_string(),
        })?;
        return Ok(Action::SendFile(text));
    }
    if s == "accept" {
        return Ok(Action::Accept);
    }
    if s == "reject" {
        return Ok(Action::Reject);
    }
    if s == "disconnect" {
        return Ok(Action::Disconnect);
    }

    let keyword = s.split_whitespace().next().unwrap_or(s);
    Err(ParseError {
        line,
        message: format!("unknown keyword '{}'", keyword),
    })
}

pub struct EventContext {
    pub message: Option<String>,
    pub file_name: Option<String>,
    pub file_size: Option<u64>,
    pub fingerprint: Option<String>,
    pub bot_start: Option<std::time::Instant>,
    pub connection_count: u64,
}

impl EventContext {
    pub fn new_with_bot_state(
        fingerprint: Option<String>,
        bot_start: Option<std::time::Instant>,
        connection_count: u64,
    ) -> Self {
        EventContext {
            message: None,
            file_name: None,
            file_size: None,
            fingerprint,
            bot_start,
            connection_count,
        }
    }
}

pub fn expand_variables(template: &str, ctx: &EventContext) -> String {
    let mut result = template.to_string();

    if let Some(ref msg) = ctx.message {
        result = result.replace("${message}", msg);
        result = result.replace("${message_length}", &msg.len().to_string());
        result = result.replace("${message_upper}", &msg.to_uppercase());
        result = result.replace("${message_lower}", &msg.to_lowercase());
        result = result.replace("${message_trimmed}", msg.trim());
        let wc = msg.split_whitespace().count();
        result = result.replace("${message_words}", &wc.to_string());
        let rev: String = msg.chars().rev().collect();
        result = result.replace("${message_reversed}", &rev);
    } else {
        result = result.replace("${message}", "");
        result = result.replace("${message_length}", "0");
        result = result.replace("${message_upper}", "");
        result = result.replace("${message_lower}", "");
        result = result.replace("${message_trimmed}", "");
        result = result.replace("${message_words}", "0");
        result = result.replace("${message_reversed}", "");
    }

    if let Some(ref name) = ctx.file_name {
        result = result.replace("${file_name}", name);
        let ext = std::path::Path::new(name)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        result = result.replace("${file_ext}", &ext);
    } else {
        result = result.replace("${file_name}", "");
        result = result.replace("${file_ext}", "");
    }
    if let Some(size) = ctx.file_size {
        result = result.replace("${file_size}", &size.to_string());
        result = result.replace("${file_size_fmt}", &format_size_human(size));
    } else {
        result = result.replace("${file_size}", "0");
        result = result.replace("${file_size_fmt}", "0 B");
    }

    let has_time_var = result.contains("${time}")
        || result.contains("${date}")
        || result.contains("${datetime}")
        || result.contains("${timestamp}")
        || result.contains("${day}")
        || result.contains("${month}")
        || result.contains("${year}")
        || result.contains("${hour}")
        || result.contains("${minute}")
        || result.contains("${second}")
        || result.contains("${weekday}")
        || result.contains("${iso8601}")
        || result.contains("${time12}")
        || result.contains("${unix}");
    if has_time_var {
        let now = chrono::Local::now();
        result = result.replace("${time}", &now.format("%H:%M:%S").to_string());
        result = result.replace("${time12}", &now.format("%I:%M:%S %p").to_string());
        result = result.replace("${date}", &now.format("%Y-%m-%d").to_string());
        result = result.replace("${datetime}", &now.format("%Y-%m-%d %H:%M:%S").to_string());
        result = result.replace("${iso8601}", &now.format("%Y-%m-%dT%H:%M:%S%z").to_string());
        result = result.replace("${timestamp}", &now.timestamp().to_string());
        result = result.replace("${unix}", &now.timestamp().to_string());
        result = result.replace("${day}", &now.format("%d").to_string());
        result = result.replace("${month}", &now.format("%m").to_string());
        result = result.replace("${year}", &now.format("%Y").to_string());
        result = result.replace("${hour}", &now.format("%H").to_string());
        result = result.replace("${minute}", &now.format("%M").to_string());
        result = result.replace("${second}", &now.format("%S").to_string());
        result = result.replace("${weekday}", &now.format("%A").to_string());
    }

    if let Some(ref fp) = ctx.fingerprint {
        result = result.replace("${fingerprint}", fp);
    } else {
        result = result.replace("${fingerprint}", "");
    }

    if let Some(start) = ctx.bot_start {
        let elapsed = start.elapsed();
        let secs = elapsed.as_secs();
        let hours = secs / 3600;
        let minutes = (secs % 3600) / 60;
        let seconds = secs % 60;
        let uptime_str = if hours > 0 {
            format!("{}h {}m {}s", hours, minutes, seconds)
        } else if minutes > 0 {
            format!("{}m {}s", minutes, seconds)
        } else {
            format!("{}s", seconds)
        };
        result = result.replace("${uptime}", &uptime_str);
        result = result.replace("${uptime_secs}", &secs.to_string());
    } else {
        result = result.replace("${uptime}", "0s");
        result = result.replace("${uptime_secs}", "0");
    }

    result = result.replace("${connections}", &ctx.connection_count.to_string());

    result = result.replace("${version}", env!("CARGO_PKG_VERSION"));

    if result.contains("${random}") {
        let n: u32 = rand::Rng::gen_range(&mut rand::thread_rng(), 0..100);
        result = result.replace("${random}", &n.to_string());
    }
    if result.contains("${random1000}") {
        let n: u32 = rand::Rng::gen_range(&mut rand::thread_rng(), 0..1000);
        result = result.replace("${random1000}", &n.to_string());
    }
    if result.contains("${uuid}") {
        let bytes: Vec<u8> = (0..16)
            .map(|_| rand::Rng::r#gen::<u8>(&mut rand::thread_rng()))
            .collect();
        let uuid = format!(
            "{}-{}-{}-{}-{}",
            hex::encode(&bytes[0..4]),
            hex::encode(&bytes[4..6]),
            hex::encode(&bytes[6..8]),
            hex::encode(&bytes[8..10]),
            hex::encode(&bytes[10..16])
        );
        result = result.replace("${uuid}", &uuid);
    }

    result
}

fn format_size_human(bytes: u64) -> String {
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


pub fn eval_condition(cond: &Condition, ctx: &EventContext) -> bool {
    match cond {
        Condition::Contains(text) => ctx
            .message
            .as_ref()
            .map_or(false, |m| m.contains(text.as_str())),
        Condition::StartsWith(text) => ctx
            .message
            .as_ref()
            .map_or(false, |m| m.starts_with(text.as_str())),
        Condition::EndsWith(text) => ctx
            .message
            .as_ref()
            .map_or(false, |m| m.ends_with(text.as_str())),
        Condition::Equals(text) => ctx.message.as_ref().map_or(false, |m| m == text),
        Condition::Not(inner) => !eval_condition(inner, ctx),
        Condition::FileSizeGt(n) => ctx.file_size.map_or(false, |s| s > *n),
        Condition::FileSizeLt(n) => ctx.file_size.map_or(false, |s| s < *n),
        Condition::FileNameEndsWith(text) => ctx
            .file_name
            .as_ref()
            .map_or(false, |n| n.ends_with(text.as_str())),
        Condition::FileNameStartsWith(text) => ctx
            .file_name
            .as_ref()
            .map_or(false, |n| n.starts_with(text.as_str())),
        Condition::FileNameContains(text) => ctx
            .file_name
            .as_ref()
            .map_or(false, |n| n.contains(text.as_str())),
        Condition::FileNameEquals(text) => ctx.file_name.as_ref().map_or(false, |n| n == text),
        Condition::MessageLengthGt(n) => ctx.message.as_ref().map_or(false, |m| m.len() > *n),
        Condition::MessageLengthLt(n) => ctx.message.as_ref().map_or(false, |m| m.len() < *n),
        Condition::MessageLengthEq(n) => ctx.message.as_ref().map_or(false, |m| m.len() == *n),
    }
}
