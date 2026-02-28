use std::env;
use std::error::Error;

use arti_client::config::TorClientConfigBuilder;
use arti_client::{StreamPrefs, TorClient, TorClientConfig};
use futures::StreamExt;
use safelog::DisplayRedacted;
use tokio::io::{AsyncBufReadExt, BufReader};
use tor_cell::relaycell::msg::Connected;
use tor_hsservice::config::OnionServiceConfigBuilder;
use tor_hsservice::handle_rend_requests;
use tor_rtcompat::PreferredRuntime;

mod config;
mod noise_peer;
mod storage;

use noise_peer::NoisePeer;
use storage::{MessageDirection, Storage};

const PATTERN: &str = "Noise_NN_25519_ChaChaPoly_BLAKE2s";

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

async fn chat_loop<T>(mut np: NoisePeer<T>, storage: Option<&Storage>) -> Result<(), Box<dyn Error>>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sized + 'static,
{
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        tokio::select! {
            result = np.recv() => {
                match result {
                    Ok(msg) => {
                        println!("peer: {}", String::from_utf8_lossy(&msg));
                        if let Some(s) = storage {
                            if let Err(e) = s.save_message(MessageDirection::Received, &msg) {
                                eprintln!("failed to save message: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("connection closed: {}", e);
                        break;
                    }
                }
            }
            line = lines.next_line() => {
                match line? {
                    Some(text) => {
                        if let Err(e) = np.send(text.as_bytes()).await {
                            eprintln!("send failed: {}", e);
                            break;
                        }
                        if let Some(s) = storage {
                            if let Err(e) = s.save_message(MessageDirection::Sent, text.as_bytes()) {
                                eprintln!("failed to save message: {}", e);
                            }
                        }
                    }
                    None => break,
                }
            }
        }
    }

    Ok(())
}

fn print_history(storage: &Storage) -> Result<(), Box<dyn Error>> {
    let messages = storage.load_history()?;
    if messages.is_empty() {
        return Ok(());
    }
    println!("### start history ###");
    for msg in messages {
        let direction = match msg.direction {
            MessageDirection::Sent => "you",
            MessageDirection::Received => "peer",
        };
        println!(
            "[{}] {}: {}",
            msg.timestamp,
            direction,
            String::from_utf8_lossy(&msg.content)
        );
    }
    println!("### end history ###");
    Ok(())
}

async fn run_initiator(
    tor: &TorClient<PreferredRuntime>,
    peer_onion: &str,
    storage: Option<Storage>,
) -> Result<(), Box<dyn Error>> {
    if let Some(ref s) = storage {
        print_history(s)?;
    }

    println!("connecting to {}...", peer_onion);
    let mut prefs = StreamPrefs::new();
    prefs.connect_to_onion_services(arti_client::config::BoolOrAuto::Explicit(true));
    let stream = tor
        .connect_with_prefs((peer_onion, 9999u16), &prefs)
        .await?;

    let np = NoisePeer::connect(stream, PATTERN).await.map_err(|e| {
        eprintln!("initiator handshake failed: {}", e);
        e
    })?;

    println!("connected. type to chat, ctrl+d to quit.");
    chat_loop(np, storage.as_ref()).await
}

async fn run_responder(
    tor: &TorClient<PreferredRuntime>,
    storage: Option<Storage>,
) -> Result<(), Box<dyn Error>> {
    if let Some(ref s) = storage {
        print_history(s)?;
    }

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

    println!("your address:");
    println!("{}", onion_addr.display_unredacted());
    println!("share this with your peer.");

    let mut stream_requests = handle_rend_requests(rend_requests);

    if let Some(stream_request) = stream_requests.next().await {
        let data_stream = stream_request.accept(Connected::new_empty()).await?;

        let np = NoisePeer::accept(data_stream, PATTERN).await.map_err(|e| {
            eprintln!("responder handshake failed: {}", e);
            e
        })?;

        println!("peer connected. type to chat, ctrl+d to quit.");
        chat_loop(np, storage.as_ref()).await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: {} (initiate <onion_addr> | listen)", args[0]);
        std::process::exit(2);
    }

    // load or create config
    let cfg = config::load_or_create()?;
    let passphrase = config::resolve_passphrase(&cfg)?;

    // open storage if persistence is enabled
    let storage = match passphrase {
        Some(ref p) if cfg.identity.persist => Some(Storage::open(p)?),
        _ => None,
    };

    // build tor config — persistent state dir if identity.persist
    let tor_config = build_tor_config(cfg.identity.persist)?;

    println!("bootstrapping tor...");
    let tor = TorClient::<PreferredRuntime>::create_bootstrapped(tor_config).await?;

    match args[1].as_str() {
        "initiate" => {
            if args.len() < 3 {
                eprintln!("usage: {} initiate <onion_addr>", args[0]);
                std::process::exit(2);
            }
            run_initiator(&tor, &args[2], storage).await?;
        }
        "listen" => {
            run_responder(&tor, storage).await?;
        }
        _ => {
            eprintln!("unknown mode: {}", args[1]);
            std::process::exit(2);
        }
    }

    Ok(())
}
