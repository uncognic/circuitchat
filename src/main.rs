use std::env;
use std::error::Error;

use arti_client::config::TorClientConfigBuilder;
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
mod noise_peer;
mod storage;
mod tui;

use noise_peer::NoisePeer;
use storage::{MessageDirection, Storage};

const PATTERN: &str = "Noise_NN_25519_ChaChaPoly_BLAKE2s";
const CONNECT_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(10);

fn build_tor_config(persist: bool) -> Result<TorClientConfig, Box<dyn Error>> {
    if !persist {
        return Ok(TorClientConfig::default());
    }

    let exe_dir = std::env::current_exe()?
        .parent()
        .ok_or("could not determine exe directory")?
        .to_path_buf();

    let config =
        TorClientConfigBuilder::from_directories(exe_dir.join("state"), exe_dir.join("cache"))
            .build()?;

    Ok(config)
}

async fn chat_loop<T>(
    mut np: NoisePeer<T>,
    storage: Option<&Storage>,
    initial_status: &str,
    time_local: bool,
    hour24: bool,
) -> Result<(), Box<dyn Error>>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sized + 'static,
{
    let mut terminal = ratatui::init();
    let mut app = tui::App::new(initial_status);

    if let Some(s) = storage {
        if let Ok(messages) = s.load_history() {
            for msg in messages {
                app.add_message(
                    msg.direction,
                    String::from_utf8_lossy(&msg.content).to_string(),
                    tui::format_timestamp(msg.timestamp, time_local, hour24),
                );
            }
        }
    }

    let mut events = EventStream::new();
    let mut incoming_file: Option<file_transfer::IncomingFile> = None;
    let mut outgoing_file: Option<file_transfer::OutgoingFile> = None;
    let mut pending_offer: Option<file_transfer::OutgoingFile> = None;

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
                    tui::now_timestamp(time_local, hour24),
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
                            tui::now_timestamp(time_local, hour24),
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
                        tui::now_timestamp(time_local, hour24),
                    );
                    app.clear_send_progress();
                }
                Err(e) => {
                    app.add_message(
                        MessageDirection::Sent,
                        format!("[file] read error: {}", e),
                        tui::now_timestamp(time_local, hour24),
                    );
                    app.clear_send_progress();
                    outgoing_file = None;
                }
            }
            continue;
        }
        // normal mode
        tokio::select! {
            result = np.recv() => {
                match result {
                    Ok(msg) => {
                        match file_transfer::parse_message(&msg) {

                            file_transfer::ParsedMessage::Text(content) => {
                                app.add_message(
                                    MessageDirection::Received,
                                    content,
                                    tui::now_timestamp(time_local, hour24),
                                );
                                if let Some(s) = storage {
                                    if let Err(e) = s.save_message(MessageDirection::Received, &msg) {
                                        app.status = format!("save error: {}", e);
                                    }
                                }
                            }
                            file_transfer::ParsedMessage::FileOffer { name, size } => {
                                let size_str = file_transfer::format_size(size);
                                app.add_message(
                                    MessageDirection::Received,
                                    format!(
                                        "[file] peer wants to send {} ({}) — type /accept or /reject",
                                        name, size_str
                                    ),
                                    tui::now_timestamp(time_local, hour24),
                                );
                                app.pending_incoming_offer = Some((name, size));
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
                                                tui::now_timestamp(time_local, hour24),
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
                                        tui::now_timestamp(time_local, hour24),
                                    );
                                    app.status = "transfer cancelled by peer".to_string();
                                    app.clear_recv_progress();
                                }
                            }
                            file_transfer::ParsedMessage::FileAccept => {
                                if let Some(out) = pending_offer.take() {
                                    app.add_message(
                                        MessageDirection::Received,
                                        format!("[file] peer accepted {}", out.name),
                                        tui::now_timestamp(time_local, hour24),
                                    );
                                    app.set_send_progress(out.name.clone(), out.size);
                                    outgoing_file = Some(out);
                                }
                            }
                            file_transfer::ParsedMessage::FileReject => {
                                if let Some(out) = pending_offer.take() {
                                    app.add_message(
                                        MessageDirection::Received,
                                        format!("[file] peer rejected {}", out.name),
                                        tui::now_timestamp(time_local, hour24),
                                    );
                                }
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
                        if let Some(text) = app.handle_key(key) {
                            if text.starts_with("/send ") {
                                let path = text[6..].trim();
                                match file_transfer::OutgoingFile::open(path) {
                                    Ok(out) => {
                                        if let Err(e) = np.send(
                                            &file_transfer::encode_offer(&out.name, out.size),
                                        ).await {
                                            app.status = format!("send failed: {}", e);
                                        } else {
                                            app.add_message(
                                                MessageDirection::Sent,
                                                format!(
                                                    "[file] offered {} ({}) — waiting for peer to accept",
                                                    out.name,
                                                    file_transfer::format_size(out.size)
                                                ),
                                                tui::now_timestamp(time_local, hour24),
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
                                        tui::now_timestamp(time_local, hour24),
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
                                    if let Err(e) = np.send(&file_transfer::encode_accept()).await {
                                        app.status = format!("send failed: {}", e);
                                    } else {
                                        match file_transfer::IncomingFile::begin(&name, size) {
                                            Ok(inc) => {
                                                app.set_recv_progress(name.clone(), size);
                                                incoming_file = Some(inc);
                                                app.pending_incoming_offer = None;
                                                app.add_message(
                                                    MessageDirection::Sent,
                                                    format!("[file] accepted {}", name),
                                                    tui::now_timestamp(time_local, hour24),
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
                                        tui::now_timestamp(time_local, hour24),
                                    );
                                } else {
                                    app.status = "no pending file offer".to_string();
                                }
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
                                    tui::now_timestamp(time_local, hour24),
                                );
                                if let Some(s) = storage {
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
    storage: Option<Storage>,
    time_local: bool,
    hour24: bool,
    auth_enabled: bool,
    password: String,
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
                return chat_loop(
                    np,
                    storage.as_ref(),
                    &format!("connected to peer {}", peer_onion),
                    time_local,
                    hour24,
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
    storage: Option<Storage>,
    time_local: bool,
    hour24: bool,
    auth_enabled: bool,
    password: String,
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
        let auth_pw = if auth_enabled {
            Some(password.clone())
        } else {
            None
        };
        if let Err(e) = np.auth_responder(auth_pw.as_deref()).await {
            eprintln!("authentication failed: {}", e);
            continue;
        }
        let status = format!("connected | you are {}", addr_str);

        if let Err(e) = chat_loop(np, storage.as_ref(), &status, time_local, hour24).await {
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

    if args.len() < 2 {
        eprintln!(
            "usage: {} (initiate <onion_addr> | listen) [--reset]",
            args[0]
        );
        std::process::exit(2);
    }

    let cfg = config::load_or_create()?;
    let passphrase = config::resolve_passphrase(&cfg)?;
    let auth_password = config::resolve_auth_password(&cfg)?;

    let storage = match passphrase {
        Some(ref p) if cfg.identity.persist => Some(Storage::open(p)?),
        _ => None,
    };

    let tor_config = build_tor_config(cfg.identity.persist)?;

    println!("bootstrapping tor...");
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
                cfg.auth.enabled,
                auth_password.unwrap_or_default(),
            )
            .await?;
        }
        "listen" => {
            run_responder(
                &tor,
                storage,
                cfg.time.local,
                cfg.time.hour24,
                cfg.auth.enabled,
                auth_password.unwrap_or_default(),
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
