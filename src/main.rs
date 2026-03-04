use std::env;
use std::error::Error;

use arti_client::config::CfgPath;
use arti_client::{StreamPrefs, TorClient, TorClientConfig};
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind};
use futures::StreamExt;
use safelog::DisplayRedacted;
use tor_cell::relaycell::msg::Connected;
use tor_hsservice::config::OnionServiceConfigBuilder;
use tor_hsservice::handle_rend_requests;
use tor_rtcompat::PreferredRuntime;

use tor_hsservice::status::State;

mod config;
mod file_transfer;
mod fingerprint;
mod noise_peer;
mod storage;
mod tui;

use crossterm::{
    cursor::MoveTo,
    execute,
    terminal::{Clear, ClearType},
};
use noise_peer::NoisePeer;
use std::process;
use storage::{MessageDirection, Storage};
use zeroize::Zeroize;

const PATTERN: &str = "Noise_NN_25519_ChaChaPoly_BLAKE2s";
const CONNECT_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(10);

struct StatusContext {
    bootstrap_secs: Option<f64>,
    bridges_configured: usize,
    bridges_active: bool,
    identity_persist: bool,
    onion_addr: Option<String>,
    history_saving: bool,
    peer_version: Option<(u8, u8, u8)>,
}
fn build_tor_config(
    persist: bool,
    bridges: &config::BridgeConfig,
) -> Result<TorClientConfig, Box<dyn Error>> {
    let mut builder = TorClientConfig::builder();

    if persist {
        let exe_dir = std::env::current_exe()?
            .parent()
            .ok_or("could not determine exe directory")?
            .to_path_buf();
        builder
            .storage()
            .cache_dir(CfgPath::new_literal(exe_dir.join("cache")));
        builder
            .storage()
            .state_dir(CfgPath::new_literal(exe_dir.join("state")));
    }

    if bridges.enabled && !bridges.lines.is_empty() {
        for line in &bridges.lines {
            let bridge: arti_client::config::BridgeConfigBuilder = line.parse()?;
            builder.bridges().bridges().push(bridge);
        }
    }

    Ok(builder.build()?)
}

fn perform_panic_and_exit(storage: Option<Storage>) -> Result<(), Box<dyn Error>> {
    use std::io::Write;

    let exe_dir = std::env::current_exe()?
        .parent()
        .ok_or("could not determine exe directory")?
        .to_path_buf();

    if let Some(s) = storage {
        s.wipe();
    }

    let cache_dir = exe_dir.join("cache");
    if cache_dir.exists() {
        zero_directory_contents(&cache_dir);
        let _ = std::fs::remove_dir_all(&cache_dir);
    }
    let state_dir = exe_dir.join("state");
    if state_dir.exists() {
        zero_directory_contents(&state_dir);
        let _ = std::fs::remove_dir_all(&state_dir);
    }

    if let Ok(cfg_path) = crate::config::config_path() {
        if cfg_path.exists() {
            let _ = crate::storage::zero_and_delete_file(&cfg_path);
        }
    }

    let _ = crate::file_transfer::remove_downloads_dir();

    let _ = ratatui::restore();
    let _ = execute!(std::io::stdout(), Clear(ClearType::All), MoveTo(0, 0));
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    process::exit(1);
}

fn zero_directory_contents(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            zero_directory_contents(&path);
        } else if path.is_file() {
            let _ = crate::storage::zero_and_delete_file(&path);
        }
    }
}

async fn chat_loop<T>(
    mut np: NoisePeer<T>,
    storage: &mut Option<Storage>,
    initial_status: &str,
    status_ctx: &mut StatusContext,
    time_local: bool,
    hour24: bool,
    show_seconds: bool,
    show_tz: bool,
    typing_indicators: bool,
    delivery_receipts: bool,
    randomize_filenames: bool,
    message_notification_sound: bool,
    mention_notification_sound: bool,
) -> Result<(), Box<dyn Error>>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sized + 'static,
{
    let mut terminal = ratatui::init();
    let mut app = tui::App::new(
        initial_status,
        message_notification_sound,
        mention_notification_sound,
    );
    app.session_fingerprint = Some(np.session_fingerprint.clone());

    if let Some(ref s) = *storage {
        if let Ok(messages) = s.load_history() {
            for msg in messages {
                app.add_message(
                    msg.direction,
                    String::from_utf8_lossy(&msg.content).to_string(),
                    tui::format_timestamp(msg.timestamp, time_local, hour24, show_tz, show_seconds),
                );
            }
        }
    }
    // perform an initial draw to establish `visible_height`, then scroll to bottom
    terminal.draw(|f| app.draw(f))?;
    app.scroll_to_bottom();
    let _ = np.send(&file_transfer::encode_version_negotiate()).await;
    if let Ok(Ok(msg)) =
        tokio::time::timeout(std::time::Duration::from_millis(250), np.recv()).await
    {
        if let file_transfer::ParsedMessage::VersionNegotiate {
            major,
            minor,
            patch,
        } = file_transfer::parse_message(&msg)
        {
            status_ctx.peer_version = Some((major, minor, patch));
            let (our_major, our_minor, our_patch) = file_transfer::protocol_version();
            if major != our_major || minor != our_minor || patch != our_patch {
                let mut warn = format!(
                    "warning: peer protocol {}.{}.{} differs from local {}.{}.{}",
                    major, minor, patch, our_major, our_minor, our_patch
                );
                if major != our_major {
                    warn = format!("INCOMPATIBLE MAJOR VERSION - {}", warn);
                }
                app.add_message(
                    MessageDirection::System,
                    warn,
                    tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                );
            }
        }
    }
    let mut events = EventStream::new();
    let mut incoming_file: Option<file_transfer::IncomingFile> = None;
    let mut outgoing_file: Option<file_transfer::OutgoingFile> = None;
    let mut pending_offer: Option<file_transfer::OutgoingFile> = None;
    let mut last_input_empty = true;

    let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(15));
    ping_interval.reset();
    let mut last_pong = tokio::time::Instant::now();
    let ping_timeout = std::time::Duration::from_secs(45);
    let mut peer_responding = true;
    let mut awaiting_ping_response = false;
    app.add_message(
            MessageDirection::System,
            "compare the fingerprint at the bottom with your peer's. if it is the same, the connection is secure.".to_string(),
            tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
        );
    loop {
        terminal.draw(|f| app.draw(f))?;

        // file mode
        if outgoing_file.is_some() {
            let cancelled = tokio::select! {
                biased;
                event = events.next() => {
                    matches!(
                        event,
                        Some(Ok(Event::Key(crossterm::event::KeyEvent {
                            code: KeyCode::Esc,
                            kind: KeyEventKind::Press,
                            ..
                        })))
                    )
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(1)) => false,
            };

            if cancelled {
                let _ = np.send(&file_transfer::encode_cancel()).await;
                let out = outgoing_file.take().unwrap();
                app.add_message(
                    MessageDirection::Sent,
                    format!("[file] cancelled sending {}", out.name),
                    tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                );
                app.clear_send_progress();
                continue;
            }

            let result = outgoing_file.as_mut().unwrap().read_next_chunk();
            match result {
                Ok(Some(data)) => {
                    if let Err(e) = np.send(&file_transfer::encode_chunk(&data)).await {
                        app.add_message(
                            MessageDirection::Sent,
                            format!("[file] send error: {}", e),
                            tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                        );
                        app.clear_send_progress();
                        outgoing_file = None;
                    } else {
                        let sent = outgoing_file.as_ref().unwrap().sent;
                        app.update_send_progress(sent);
                    }
                }
                Ok(None) => {
                    let _ = np.send(&file_transfer::encode_done()).await;
                    let out = outgoing_file.take().unwrap();
                    app.add_message(
                        MessageDirection::Sent,
                        format!(
                            "[file] sent {} ({})",
                            out.name,
                            file_transfer::format_size(out.size)
                        ),
                        tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                    );
                    app.clear_send_progress();
                }
                Err(e) => {
                    app.add_message(
                        MessageDirection::Sent,
                        format!("[file] read error: {}", e),
                        tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                    );
                    app.clear_send_progress();
                    outgoing_file = None;
                }
            }
            continue;
        }
        // normal mode
        tokio::select! {
            _ = ping_interval.tick() => {
                if last_pong.elapsed() > ping_timeout {
                    if peer_responding {
                        peer_responding = false;
                        let base = app.status.replace(" | peer not responding", "");
                        app.status = format!("{} | peer not responding", base);
                    }
                }
                awaiting_ping_response = false;
                let _ = np.send(&file_transfer::encode_ping()).await;
            }
            result = np.recv() => {
                match result {
                    Ok(msg) => {
                        last_pong = tokio::time::Instant::now();
                        if !peer_responding {
                            peer_responding = true;
                            app.status = app.status.replace(" | peer not responding", "");
                        }
                        match file_transfer::parse_message(&msg) {
                            file_transfer::ParsedMessage::VersionNegotiate { major, minor, patch } => {
                                status_ctx.peer_version = Some((major, minor, patch));
                                let (our_major, our_minor, our_patch) = file_transfer::protocol_version();

                                if major != our_major || minor != our_minor || patch != our_patch {
                                    let mut warn = format!("warning: peer protocol {}.{}.{} differs from local {}.{}.{}", major, minor, patch, our_major, our_minor, our_patch);
                                    if major != our_major {
                                        warn = format!("INCOMPATIBLE MAJOR VERSION - {}", warn);
                                    }
                                    app.add_message(
                                        MessageDirection::System,
                                        warn,
                                        tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                    );
                                }
                            }

                            file_transfer::ParsedMessage::Text(content) => {
                                app.add_message(
                                    MessageDirection::Received,
                                    content,
                                    tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                );
                                if let Some(ref s) = *storage {
                                    if let Err(e) = s.save_message(MessageDirection::Received, &msg) {
                                        app.status = format!("save error: {}", e);
                                    }
                                }
                                if delivery_receipts {
                                    let _ = np.send(&file_transfer::encode_delivered()).await;
                                }
                                if typing_indicators {
                                    app.peer_typing = false;
                                    app.status = app.status.replace(" | peer is typing...", "");
                                }
                            }
                            file_transfer::ParsedMessage::FileOffer { name, size, checksum } => {
                                let size_str = file_transfer::format_size(size);
                                app.add_message(
                                    MessageDirection::Received,
                                    format!(
                                        "[file] peer wants to send {} ({}) - type /accept or /reject",
                                        name, size_str
                                    ),
                                    tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                );
                                app.pending_incoming_offer = Some((name, size, checksum));
                            }
                            file_transfer::ParsedMessage::FileChunk(data) => {
                                if let Some(ref mut inc) = incoming_file {
                                    if let Err(e) = inc.write_chunk(&data) {
                                        app.status = format!("file write error: {}", e);
                                        app.clear_recv_progress();
                                        incoming_file = None;
                                    } else {
                                        app.update_recv_progress(inc.received);
                                    }
                                }
                            }
                            file_transfer::ParsedMessage::FileDone => {
                                if let Some(inc) = incoming_file.take() {
                                    let name = inc.name.clone();
                                    let size = inc.size;
                                    match inc.finish() {
                                        Ok(path) => {
                                            app.add_message(
                                                MessageDirection::Received,
                                                format!(
                                                    "[file] saved {} ({}) -> {}",
                                                    name,
                                                    file_transfer::format_size(size),
                                                    path.display()
                                                ),
                                                tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                            );
                                            app.status = "file received".to_string();
                                            app.clear_recv_progress();
                                        }
                                        Err(e) => {
                                            app.status = format!("file save error: {}", e);
                                            app.clear_recv_progress();
                                        }
                                    }
                                }
                            }
                            file_transfer::ParsedMessage::FileCancel => {
                                if let Some(inc) = incoming_file.take() {
                                    inc.cancel();
                                        app.add_message(
                                        MessageDirection::Received,
                                        "[file] peer cancelled the transfer".to_string(),
                                        tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                    );
                                    app.status = "transfer cancelled by peer".to_string();
                                    app.clear_recv_progress();
                                }
                            }
                            file_transfer::ParsedMessage::FileAccept(offset) => {
                                if let Some(mut out) = pending_offer.take() {
                                    if let Err(e) = out.seek_to(offset) {
                                        app.status = format!("file seek error: {}", e);
                                    } else {
                                        app.add_message(
                                            MessageDirection::Received,
                                            format!("[file] peer accepted {}", out.name),
                                            tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                        );
                                        app.set_send_progress(out.name.clone(), out.size);
                                        app.update_send_progress(out.sent);
                                        outgoing_file = Some(out);
                                    }
                                }
                            }
                            file_transfer::ParsedMessage::FileReject => {
                                if let Some(out) = pending_offer.take() {
                                    app.add_message(
                                        MessageDirection::Received,
                                        format!("[file] peer rejected {}", out.name),
                                        tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                    );
                                }
                            }
                            file_transfer::ParsedMessage::TypingStart => {
                                if typing_indicators {
                                    app.peer_typing = true;
                                    let current = app.status.trim_end_matches(" | peer is typing...").to_string();
                                    app.status = format!("{} | peer is typing...", current);
                                }
                            }
                            file_transfer::ParsedMessage::TypingStop => {
                                if typing_indicators {
                                    app.peer_typing = false;
                                    app.status = app.status.replace(" | peer is typing...", "");
                                }
                            }
                            file_transfer::ParsedMessage::Delivered => {
                                if delivery_receipts && app.pending_delivery > 0 {
                                    app.pending_delivery -= 1;
                                    app.mark_last_sent_delivered();
                                }
                            }
                            file_transfer::ParsedMessage::Ping => {
                                let _ = np.send(&file_transfer::encode_pong()).await;
                            }
                            file_transfer::ParsedMessage::Pong => {
                                if awaiting_ping_response {
                                    app.add_message(
                                        MessageDirection::Received,
                                        "Pong!".to_string(),
                                        tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                    );
                                }
                                awaiting_ping_response = false;
                            }
                        }
                    }
                    Err(_) => {
                        app.status = "peer disconnected".to_string();
                        terminal.draw(|f| app.draw(f))?;
                        break;
                    }
                }
            }
            event = events.next() => {
                match event {
                    Some(Ok(Event::Key(key))) => {
                        let submitted = app.handle_key(key);
                        if typing_indicators {
                                let now_empty = app.input.is_empty();
                                if last_input_empty && !now_empty {
                                    let _ = np.send(&file_transfer::encode_typing_start()).await;
                                } else if !last_input_empty && now_empty {
                                    let _ = np.send(&file_transfer::encode_typing_stop()).await;
                                }
                                last_input_empty = now_empty;
                            }
                            if let Some(text) = submitted {
                            if text.starts_with("/send ") {
                                let path = text[6..].trim();
                                match file_transfer::OutgoingFile::open(path) {
                                    Ok(mut out) => {
                                        if randomize_filenames {
                                            out.name = file_transfer::randomize_filename_preserve_ext(&out.name);
                                        }
                                        if let Err(e) = np.send(
                                            &file_transfer::encode_offer_with_checksum(&out.name, out.size, Some(&out.checksum)),
                                        ).await {
                                            app.status = format!("send failed: {}", e);
                                        } else {
                                            app.add_message(
                                                MessageDirection::Sent,
                                                format!(
                                                    "[file] offered {} ({}) - waiting for peer to accept",
                                                    out.name,
                                                    file_transfer::format_size(out.size)
                                                ),
                                                tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                            );
                                            pending_offer = Some(out);
                                        }
                                    }
                                    Err(e) => {
                                        app.status = format!("cannot open file: {}", e);
                                    }
                                }
                            } else if text == "/cancel" {
                                if let Some(inc) = incoming_file.take() {
                                    inc.cancel();
                                        app.add_message(
                                            MessageDirection::Sent,
                                            "[file] cancelled receiving".to_string(),
                                            tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                        );
                                    app.status = "cancelled incoming transfer".to_string();
                                    app.clear_recv_progress();
                                } else {
                                    app.status = "no active incoming transfer".to_string();
                                }
                            } else if text == "/accept" {
                                if incoming_file.is_some() {
                                    app.status = "transfer already in progress".to_string();
                                } else if let Some(ref offer) = app.pending_incoming_offer {
                                    let name = offer.0.clone();
                                    let size = offer.1;
                                    let checksum = offer.2.as_ref();
                                    let existing = match file_transfer::existing_download_size(&name) {
                                        Ok(n) => n,
                                        Err(_) => 0,
                                    };

                                    if existing == size && checksum.is_some() {
                                        if let Ok(path) = file_transfer::download_path(&name) {
                                                if let Ok(sum) = file_transfer::file_xxh3(&path) {
                                                if &sum == checksum.unwrap() {
                                                    app.add_message(
                                                        MessageDirection::Received,
                                                        format!("[file] already downloaded {}", name),
                                                        tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                                    );
                                                    app.pending_incoming_offer = None;
                                                    continue;
                                                }
                                            }
                                        }
                                    }
                                    if let Err(e) = np
                                        .send(&file_transfer::encode_accept_with_offset(existing))
                                        .await
                                    {
                                        app.status = format!("send failed: {}", e);
                                    } else {
                                        match file_transfer::IncomingFile::begin(&name, size, offer.2.as_deref()) {
                                            Ok(inc) => {
                                                app.set_recv_progress(name.clone(), size);
                                                incoming_file = Some(inc);
                                                app.pending_incoming_offer = None;
                                                app.add_message(
                                                    MessageDirection::Sent,
                                                    format!("[file] accepted {}", name),
                                                    tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                                );
                                            }
                                            Err(e) => {
                                                app.status = format!("file receive error: {}", e);
                                            }
                                        }
                                    }
                                } else {
                                    app.status = "no pending file offer".to_string();
                                }
                            } else if text == "/reject" {
                                if let Some(ref offer) = app.pending_incoming_offer {
                                    let name = offer.0.clone();
                                    let _ = np.send(&file_transfer::encode_reject()).await;
                                    app.pending_incoming_offer = None;
                                    app.add_message(
                                        MessageDirection::Sent,
                                        format!("[file] rejected {}", name),
                                        tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                    );
                                } else {
                                    app.status = "no pending file offer".to_string();
                                }
                            } else if text == "/clear" {
                                app.messages.clear();
                            } else if text == "/panic" {
                                let owned_storage = storage.take();
                                if let Err(e) = perform_panic_and_exit(owned_storage) {
                                    eprintln!("panic cleanup failed: {}", e);
                                }
                                process::exit(1);
                            } else if text == "/help" {
                                app.add_message(
                                    MessageDirection::System,
                                    "[help] available commands: /clear, /help, /status, /send, /ping, /panic".to_string(),
                                    tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                );
                            } else if text == "/status" {
                                let ts = tui::now_timestamp(time_local, hour24, show_tz, show_seconds);
                                let bs_line = if let Some(s) = status_ctx.bootstrap_secs {
                                    format!("[status] tor: connected ({:.1}s bootstrap)", s)
                                } else {
                                    "[status] tor: connected".to_string()
                                };

                                let bridges_line = if status_ctx.bridges_active {
                                    format!(
                                        "[status] bridges: active ({} configured)",
                                        status_ctx.bridges_configured
                                    )
                                } else {
                                    "[status] bridges: not configured".to_string()
                                };
                                if let Some(ref addr) = status_ctx.onion_addr {
                                    app.add_message(
                                        MessageDirection::System,
                                        format!("[status] your address: {}", addr),
                                        ts.clone(),
                                    );
                                }
                                let peer_version_str = if let Some((maj, min, pat)) = status_ctx.peer_version {
                                    format!("{}.{}.{}", maj, min, pat)
                                } else {
                                    "unknown".to_string()
                                };
                                app.add_message(
                                    MessageDirection::System,
                                    format!("[status] peer protocol version: {}", peer_version_str),
                                    tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                );
                                let (our_major, our_minor, our_patch) = file_transfer::protocol_version();
                                app.add_message(
                                    MessageDirection::System,
                                    format!("[status] protocol version: {}.{}.{}", our_major, our_minor, our_patch),
                                    tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                );
                                let identity_line = if status_ctx.identity_persist {
                                    "[status] identity: persistent".to_string()
                                } else {
                                    "[status] identity: ephemeral".to_string()
                                };

                                let history_line = if status_ctx.history_saving {
                                    "[status] history: saving (encrypted)".to_string()
                                } else {
                                    "[status] history: disabled".to_string()
                                };

                                app.add_message(
                                    MessageDirection::System,
                                    bs_line,
                                    ts.clone(),
                                );
                                app.add_message(
                                    MessageDirection::System,
                                    bridges_line,
                                    ts.clone(),
                                );
                                app.add_message(
                                    MessageDirection::System,
                                    identity_line,
                                    ts.clone(),
                                );
                                app.add_message(
                                    MessageDirection::System,
                                    history_line,
                                    ts,
                                );
                            } else if text == "/ping" {
                                awaiting_ping_response = true;
                                app.add_message(
                                    MessageDirection::Sent,
                                    "Ping?".to_string(),
                                    tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                );
                                let _ = np.send(&file_transfer::encode_ping()).await;
                            } else {
                                let bytes = text.as_bytes().to_vec();
                                if let Err(e) = np.send(&bytes).await {
                                    app.status = format!("send failed: {}", e);
                                    terminal.draw(|f| app.draw(f))?;
                                    break;
                                }
                                app.add_message(
                                    MessageDirection::Sent,
                                    text,
                                    tui::now_timestamp(time_local, hour24, show_tz, show_seconds),
                                );
                                if typing_indicators {
                                    let _ = np.send(&file_transfer::encode_typing_stop()).await;
                                    last_input_empty = true;
                                }
                                if delivery_receipts {
                                    app.pending_delivery += 1;
                                }
                                if let Some(ref s) = *storage {
                                    if let Err(e) = s.save_message(MessageDirection::Sent, &bytes) {
                                        app.status = format!("save error: {}", e);
                                    }
                                }
                            }
                        }
                        if app.should_quit {
                            break;
                        }
                    }
                    Some(Ok(Event::Resize(_, _))) => {}
                    Some(Err(_)) | None => break,
                    _ => {}
                }
            }
        }
    }

    ratatui::restore();
    Ok(())
}

async fn run_initiator(
    tor: &TorClient<PreferredRuntime>,
    peer_onion: &str,
    mut storage: Option<Storage>,
    time_local: bool,
    hour24: bool,
    show_seconds: bool,
    show_tz: bool,
    auth_enabled: bool,
    password: String,
    typing_indicators: bool,
    delivery_receipts: bool,
    randomize_filenames: bool,
    message_notification_sound: bool,
    mention_notification_sound: bool,
) -> Result<(), Box<dyn Error>> {
    let mut prefs = StreamPrefs::new();
    prefs.connect_to_onion_services(arti_client::config::BoolOrAuto::Explicit(true));

    let start = std::time::Instant::now();
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        if attempt == 1 {
            println!("connecting to {}...", peer_onion);
        } else {
            println!(
                "[{:.1}s] retrying (attempt {})... peer may still be publishing its descriptor",
                start.elapsed().as_secs_f64(),
                attempt
            );
        }

        match tor.connect_with_prefs((peer_onion, 9999u16), &prefs).await {
            Ok(stream) => {
                println!("connected in {:.1}s", start.elapsed().as_secs_f64());
                let mut np = NoisePeer::connect(stream, PATTERN).await.map_err(|e| {
                    eprintln!("initiator handshake failed: {}", e);
                    e
                })?;
                let auth_pw = if auth_enabled {
                    Some(password.clone())
                } else {
                    None
                };
                np.auth_initiator(auth_pw.as_deref()).await?;
                let mut status_ctx = StatusContext {
                    bootstrap_secs: None,
                    bridges_configured: 0,
                    bridges_active: false,
                    identity_persist: storage.is_some(),
                    onion_addr: None,
                    history_saving: storage.is_some(),
                    peer_version: None,
                };
                let mut password_owned = password;
                password_owned.zeroize();
                let initial_status = "connected";

                return chat_loop(
                    np,
                    &mut storage,
                    &initial_status,
                    &mut status_ctx,
                    time_local,
                    hour24,
                    show_seconds,
                    show_tz,
                    typing_indicators,
                    delivery_receipts,
                    randomize_filenames,
                    message_notification_sound,
                    mention_notification_sound,
                )
                .await;
            }
            Err(e) => {
                eprintln!(
                    "[{:.1}s] attempt {} failed: {}",
                    start.elapsed().as_secs_f64(),
                    attempt,
                    e
                );
                tokio::time::sleep(CONNECT_RETRY_DELAY).await;
            }
        }
    }
}

async fn run_responder(
    tor: &TorClient<PreferredRuntime>,
    mut storage: Option<Storage>,
    time_local: bool,
    hour24: bool,
    show_seconds: bool,
    show_tz: bool,
    auth_enabled: bool,
    password: String,
    typing_indicators: bool,
    delivery_receipts: bool,
    randomize_filenames: bool,
    message_notification_sound: bool,
    mention_notification_sound: bool,
) -> Result<(), Box<dyn Error>> {
    let config = OnionServiceConfigBuilder::default()
        .nickname("circuitchat".to_owned().try_into()?)
        .build()?;

    let (service, rend_requests) = tor
        .launch_onion_service(config)?
        .ok_or("onion services disabled in config")?;

    let onion_addr = loop {
        if let Some(addr) = service.onion_address() {
            break addr;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    };

    let addr_str = format!("{}", onion_addr.display_unredacted());
    println!("your address: {}", addr_str);
    println!("publishing descriptor to the tor network...");

    let start = std::time::Instant::now();
    let mut status_events = service.status_events();
    let mut last_state = None;

    loop {
        if service.status().state().is_fully_reachable() {
            break;
        }

        match tokio::time::timeout(std::time::Duration::from_secs(10), status_events.next()).await {
            Ok(Some(status)) => {
                let state = status.state();
                match state {
                    State::Running | State::DegradedReachable => break,
                    State::Broken => {
                        return Err(format!(
                            "onion service broken: {:?}",
                            status.current_problem()
                        )
                        .into());
                    }
                    other => {
                        if last_state != Some(other) {
                            println!(
                                "[{:.1}s] service state: {:?}",
                                start.elapsed().as_secs_f64(),
                                other
                            );
                            last_state = Some(other);
                        }
                    }
                }
            }
            Ok(None) => return Err("status stream ended unexpectedly".into()),
            Err(_) => {
                println!(
                    "[{:.1}s] still waiting for descriptor publication...",
                    start.elapsed().as_secs_f64()
                );
            }
        }
    }

    println!(
        "descriptor published in {:.1}s, service is reachable",
        start.elapsed().as_secs_f64()
    );
    println!("share your address with your peer. waiting for connection...");

    let mut stream_requests = handle_rend_requests(rend_requests);

    while let Some(stream_request) = stream_requests.next().await {
        let data_stream = match stream_request.accept(Connected::new_empty()).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("failed to accept incoming connection: {}", e);
                continue;
            }
        };

        let mut np = match NoisePeer::accept(data_stream, PATTERN).await {
            Ok(n) => n,
            Err(e) => {
                eprintln!("responder handshake failed: {}", e);
                continue;
            }
        };
        let mut auth_pw = if auth_enabled {
            Some(password.clone())
        } else {
            None
        };
        if let Err(e) = np.auth_responder(auth_pw.as_deref()).await {
            if let Some(ref mut p) = auth_pw {
                p.zeroize();
            }
            eprintln!("authentication failed: {}", e);
            continue;
        }
        if let Some(ref mut p) = auth_pw {
            p.zeroize();
        }
        let status = "connected".to_string();

        let mut status_ctx = StatusContext {
            bootstrap_secs: None,
            bridges_configured: 0,
            bridges_active: false,
            identity_persist: storage.is_some(),
            onion_addr: Some(addr_str.clone()),
            history_saving: storage.is_some(),
            peer_version: None,
        };

        if let Err(e) = chat_loop(
            np,
            &mut storage,
            &status,
            &mut status_ctx,
            time_local,
            hour24,
            show_seconds,
            show_tz,
            typing_indicators,
            delivery_receipts,
            randomize_filenames,
            message_notification_sound,
            mention_notification_sound,
        )
        .await
        {
            eprintln!("chat loop ended with error: {}", e);
        } else {
            println!("peer disconnected, waiting for next connection...");
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() >= 2 && args[1] == "--reset" {
        if let Err(e) = (|| -> Result<(), Box<dyn Error>> {
            let exe_dir = std::env::current_exe()?
                .parent()
                .ok_or("could not determine exe directory")?
                .to_path_buf();

            let db = exe_dir.join("circuitchat.db");
            if db.exists() {
                std::fs::remove_file(&db)?;
                println!("deleted {}", db.display());
            }

            let cache = exe_dir.join("cache");
            if cache.exists() {
                std::fs::remove_dir_all(&cache)?;
                println!("deleted {}", cache.display());
            }

            let state = exe_dir.join("state");
            if state.exists() {
                std::fs::remove_dir_all(&state)?;
                println!("deleted {}", state.display());
            }

            Ok(())
        })() {
            eprintln!("reset failed: {}", e);
            std::process::exit(1);
        }
        println!("state reset complete");
        return Ok(());
    }

    if args.iter().any(|a| a == "--version") {
        println!("circuitchat v{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args.len() < 2 {
        eprintln!(
            "usage: {} (initiate <onion_addr> | listen) [--reset, --version]",
            args[0]
        );
        std::process::exit(2);
    }
    println!("circuitchat v{}", env!("CARGO_PKG_VERSION"));
    let cfg = config::load_or_create()?;
    let mut passphrase = config::resolve_passphrase(&cfg)?;
    let auth_password = config::resolve_auth_password(&cfg)?;

    let storage = match passphrase {
        Some(ref p) if cfg.identity.persist => Some(Storage::open(p)?),
        _ => None,
    };

    if let Some(ref mut p) = passphrase {
        p.zeroize();
    }

    let tor_config = build_tor_config(cfg.identity.persist, &cfg.bridge)?;

    println!("bootstrapping tor...");
    if cfg.bridge.enabled && !cfg.bridge.lines.is_empty() {
        println!("bridges: active ({} configured)", cfg.bridge.lines.len());
    } else {
        println!("bridges: not configured");
    }
    let start = std::time::Instant::now();
    let tor = TorClient::<PreferredRuntime>::create_bootstrapped(tor_config).await?;
    let elapsed = start.elapsed();
    println!("tor bootstrapped in {:.1}s", elapsed.as_secs_f64());

    if elapsed.as_secs() < 2 {
        println!("(note: tor bootstrap was fast, probably using cached tor state)");
    }

    match args[1].as_str() {
        "initiate" => {
            if args.len() < 3 {
                eprintln!("usage: {} initiate <onion_addr>", args[0]);
                std::process::exit(2);
            }
            run_initiator(
                &tor,
                &args[2],
                storage,
                cfg.time.local,
                cfg.time.hour24,
                cfg.time.show_seconds,
                cfg.time.show_tz,
                cfg.auth.enabled,
                auth_password.unwrap_or_default(),
                cfg.privacy.typing_status,
                cfg.privacy.read_receipts,
                cfg.privacy.randomize_filenames,
                cfg.ui.message_notification_sound,
                cfg.ui.mention_notification_sound,
            )
            .await?;
        }
        "listen" => {
            run_responder(
                &tor,
                storage,
                cfg.time.local,
                cfg.time.hour24,
                cfg.time.show_seconds,
                cfg.time.show_tz,
                cfg.auth.enabled,
                auth_password.unwrap_or_default(),
                cfg.privacy.typing_status,
                cfg.privacy.read_receipts,
                cfg.privacy.randomize_filenames,
                cfg.ui.message_notification_sound,
                cfg.ui.mention_notification_sound,
            )
            .await?;
        }
        _ => {
            eprintln!("unknown mode: {}", args[1]);
            std::process::exit(2);
        }
    }

    Ok(())
}
