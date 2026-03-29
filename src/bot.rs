use std::error::Error;

use crate::ccscript::{self, Action, Block, Event, EventContext, Script};
use crate::files;
use crate::noise_peer::NoisePeer;

pub struct ActionOutcome {
    pub replies: Vec<String>,
    pub waits: Vec<u64>,
    pub accept_file: bool,
    pub reject_file: bool,
    pub send_files: Vec<String>,
    pub disconnect: bool,
}

fn run_handlers(script: &Script, event: &Event, ctx: &EventContext) -> ActionOutcome {
    let mut outcome = ActionOutcome {
        replies: Vec::new(),
        waits: Vec::new(),
        accept_file: false,
        reject_file: false,
        send_files: Vec::new(),
        disconnect: false,
    };

    for handler in &script.handlers {
        if handler.event != *event {
            continue;
        }

        for block in &handler.blocks {
            match block {
                Block::Conditional { condition, actions } => {
                    if ccscript::eval_condition(condition, ctx) {
                        for action in actions {
                            apply_action(action, ctx, &mut outcome, event);
                        }
                    }
                }
                Block::Unconditional(action) => {
                    apply_action(action, ctx, &mut outcome, event);
                }
            }
        }
    }

    outcome
}

fn apply_action(action: &Action, ctx: &EventContext, outcome: &mut ActionOutcome, event: &Event) {
    match action {
        Action::Reply(template) => {
            let text = ccscript::expand_variables(template, ctx);
            outcome.replies.push(text);
        }
        Action::Log(template) => {
            let text = ccscript::expand_variables(template, ctx);
            println!("{}", text);
        }
        Action::Wait(ms) => {
            outcome.waits.push(*ms);
        }
        Action::SendFile(path_template) => {
            let path = ccscript::expand_variables(path_template, ctx);
            outcome.send_files.push(path);
        }
        Action::Accept => {
            if *event == Event::File {
                outcome.accept_file = true;
            } else {
                eprintln!("runtime error: 'accept' used outside file event, skipping");
            }
        }
        Action::Reject => {
            if *event == Event::File {
                outcome.reject_file = true;
            } else {
                eprintln!("runtime error: 'reject' used outside file event, skipping");
            }
        }
        Action::Disconnect => {
            outcome.disconnect = true;
        }
    }
}

async fn send_outcome<T>(
    np: &mut NoisePeer<T>,
    outcome: &ActionOutcome,
) -> Result<(), Box<dyn Error>>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    for ms in &outcome.waits {
        tokio::time::sleep(std::time::Duration::from_millis(*ms)).await;
    }
    for reply in &outcome.replies {
        np.send(reply.as_bytes()).await?;
    }
    Ok(())
}

pub async fn run_bot_session<T>(
    mut np: NoisePeer<T>,
    script: &Script,
    bot_start: std::time::Instant,
    connection_count: u64,
) -> Result<(), Box<dyn Error>>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let fingerprint = np.session_fingerprint.clone();

    let _ = np.send(&files::encode_version_negotiate()).await;

    if let Ok(Ok(msg)) =
        tokio::time::timeout(std::time::Duration::from_millis(250), np.recv()).await
    {
        if let files::ParsedMessage::VersionNegotiate {
            major,
            minor: _,
            patch: _,
        } = files::parse_message(&msg)
        {
            let (our_major, _, _) = files::protocol_version();
            if major != our_major {
                eprintln!("warning: peer has incompatible major protocol version");
            }
        }
    }

    let ctx = EventContext::new_with_bot_state(
        Some(fingerprint.clone()),
        Some(bot_start),
        connection_count,
    );
    let outcome = run_handlers(script, &Event::Connect, &ctx);
    send_outcome(&mut np, &outcome).await?;
    for path in &outcome.send_files {
        match files::OutgoingFile::open(path) {
            Ok(out) => {
                let _ = np
                    .send(&files::encode_offer_with_checksum(
                        &out.name,
                        out.size,
                        Some(&out.checksum),
                    ))
                    .await;
                println!(
                    "[file] offered {} ({}) - waiting for peer to accept",
                    out.name,
                    files::format_size(out.size)
                );
            }
            Err(e) => eprintln!("cannot open file: {}", e),
        }
    }
    if outcome.disconnect {
        return Ok(());
    }

    let mut incoming_file: Option<files::IncomingFile> = None;
    let mut pending_offer: Option<files::OutgoingFile> = None;

    loop {
        if incoming_file.is_some() {
            match np.recv().await {
                Ok(msg) => match files::parse_message(&msg) {
                    files::ParsedMessage::FileChunk(data) => {
                        if let Some(ref mut inc) = incoming_file {
                            if let Err(e) = inc.write_chunk(&data) {
                                eprintln!("file write error: {}", e);
                                incoming_file = None;
                            }
                        }
                    }
                    files::ParsedMessage::FileDone => {
                        if let Some(inc) = incoming_file.take() {
                            let name = inc.name.clone();
                            let size = inc.size;
                            match inc.finish() {
                                Ok(path) => {
                                    println!(
                                        "[file] saved {} ({}) -> {}",
                                        name,
                                        files::format_size(size),
                                        path.display()
                                    );
                                }
                                Err(e) => eprintln!("file save error: {}", e),
                            }
                        }
                    }
                    files::ParsedMessage::FileCancel => {
                        if let Some(inc) = incoming_file.take() {
                            inc.cancel();
                            println!("[file] peer cancelled the transfer");
                        }
                    }
                    _ => {}
                },
                Err(_) => {
                    fire_disconnect(script, &mut np, &fingerprint, bot_start, connection_count)
                        .await;
                    break;
                }
            }
            continue;
        }

        match np.recv().await {
            Ok(msg) => match files::parse_message(&msg) {
                files::ParsedMessage::Text(content) => {
                    let mut ctx = EventContext::new_with_bot_state(
                        Some(fingerprint.clone()),
                        Some(bot_start),
                        connection_count,
                    );
                    ctx.message = Some(content);
                    let outcome = run_handlers(script, &Event::Message, &ctx);
                    send_outcome(&mut np, &outcome).await?;
                    for path in &outcome.send_files {
                        match files::OutgoingFile::open(path) {
                            Ok(out) => {
                                if let Err(e) = np
                                    .send(&files::encode_offer_with_checksum(
                                        &out.name,
                                        out.size,
                                        Some(&out.checksum),
                                    ))
                                    .await
                                {
                                    eprintln!("send failed: {}", e);
                                } else {
                                    println!(
                                        "[file] offered {} ({}) - waiting for peer to accept",
                                        out.name,
                                        files::format_size(out.size)
                                    );
                                    pending_offer = Some(out);
                                }
                            }
                            Err(e) => eprintln!("cannot open file: {}", e),
                        }
                    }
                    if outcome.disconnect {
                        fire_disconnect(script, &mut np, &fingerprint, bot_start, connection_count)
                            .await;
                        return Ok(());
                    }
                }
                files::ParsedMessage::FileOffer {
                    name,
                    size,
                    checksum,
                } => {
                    let mut ctx = EventContext::new_with_bot_state(
                        Some(fingerprint.clone()),
                        Some(bot_start),
                        connection_count,
                    );
                    ctx.file_name = Some(name.clone());
                    ctx.file_size = Some(size);
                    let outcome = run_handlers(script, &Event::File, &ctx);
                    send_outcome(&mut np, &outcome).await?;
                    for path in &outcome.send_files {
                        match files::OutgoingFile::open(path) {
                            Ok(out) => {
                                if let Err(e) = np
                                    .send(&files::encode_offer_with_checksum(
                                        &out.name,
                                        out.size,
                                        Some(&out.checksum),
                                    ))
                                    .await
                                {
                                    eprintln!("send failed: {}", e);
                                } else {
                                    println!(
                                        "[file] offered {} ({}) - waiting for peer to accept",
                                        out.name,
                                        files::format_size(out.size)
                                    );
                                    pending_offer = Some(out);
                                }
                            }
                            Err(e) => eprintln!("cannot open file: {}", e),
                        }
                    }
                    if outcome.accept_file {
                        let existing = files::existing_download_size(&name).unwrap_or(0);
                        np.send(&files::encode_accept_with_offset(existing))
                            .await?;
                        match files::IncomingFile::begin(&name, size, checksum.as_deref()) {
                            Ok(inc) => {
                                println!(
                                    "[file] accepted {} ({})",
                                    name,
                                    files::format_size(size)
                                );
                                incoming_file = Some(inc);
                            }
                            Err(e) => eprintln!("file receive error: {}", e),
                        }
                    } else if outcome.reject_file {
                        np.send(&files::encode_reject()).await?;
                        println!("[file] rejected {}", name);
                    }
                    if outcome.disconnect {
                        fire_disconnect(script, &mut np, &fingerprint, bot_start, connection_count)
                            .await;
                        return Ok(());
                    }
                }
                files::ParsedMessage::Ping => {
                    let _ = np.send(&files::encode_pong()).await;
                }
                files::ParsedMessage::FileAccept(offset) => {
                    if let Some(mut out) = pending_offer.take() {
                        if let Err(e) = out.seek_to(offset) {
                            eprintln!("file seek error: {}", e);
                            continue;
                        }
                        loop {
                            match out.read_next_chunk() {
                                Ok(Some(chunk)) => {
                                    if let Err(e) =
                                        np.send(&files::encode_chunk(&chunk)).await
                                    {
                                        eprintln!("file send chunk error: {}", e);
                                        break;
                                    }
                                }
                                Ok(None) => {
                                    let _ = np.send(&files::encode_done()).await;
                                    println!(
                                        "[file] sent {} ({})",
                                        out.name,
                                        files::format_size(out.size)
                                    );
                                    break;
                                }
                                Err(e) => {
                                    eprintln!("file read error: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                }
                files::ParsedMessage::FileReject => {
                    if let Some(out) = pending_offer.take() {
                        println!("[file] peer rejected {}", out.name);
                    }
                }
                files::ParsedMessage::VersionNegotiate {
                    major,
                    minor,
                    patch,
                } => {
                    let (our_major, our_minor, our_patch) = files::protocol_version();
                    if major != our_major {
                        eprintln!(
                            "warning: incompatible peer version {}.{}.{} (ours {}.{}.{})",
                            major, minor, patch, our_major, our_minor, our_patch
                        );
                    }
                }
                _ => {}
            },
            Err(_) => {
                fire_disconnect(script, &mut np, &fingerprint, bot_start, connection_count).await;
                break;
            }
        }
    }

    Ok(())
}

async fn fire_disconnect<T>(
    script: &Script,
    np: &mut NoisePeer<T>,
    fingerprint: &str,
    bot_start: std::time::Instant,
    connection_count: u64,
) where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let ctx = EventContext::new_with_bot_state(
        Some(fingerprint.to_string()),
        Some(bot_start),
        connection_count,
    );
    let outcome = run_handlers(script, &Event::Disconnect, &ctx);
    for reply in &outcome.replies {
        let _ = np.send(reply.as_bytes()).await;
    }
}
