use std::env;
use std::error::Error;

use arti_client::{StreamPrefs, TorClient, TorClientConfig};
use futures::StreamExt;
use safelog::DisplayRedacted;
use tokio::io::{AsyncBufReadExt, BufReader};
use tor_cell::relaycell::msg::Connected;
use tor_hsservice::config::OnionServiceConfigBuilder;
use tor_hsservice::handle_rend_requests;
use tor_rtcompat::PreferredRuntime;

mod noise_peer;
use noise_peer::NoisePeer;

const PATTERN: &str = "Noise_NN_25519_ChaChaPoly_BLAKE2s";

async fn chat_loop<T>(mut np: NoisePeer<T>) -> Result<(), Box<dyn Error>>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        tokio::select! {
            result = np.recv() => {
                match result {
                    Ok(msg) => println!("peer: {}", String::from_utf8_lossy(&msg)),
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
                    }
                    None => break,
                }
            }
        }
    }

    Ok(())
}

async fn run_initiator(
    tor: &TorClient<PreferredRuntime>,
    peer_onion: &str,
) -> Result<(), Box<dyn Error>> {
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
    chat_loop(np).await
}

async fn run_responder(tor: &TorClient<PreferredRuntime>) -> Result<(), Box<dyn Error>> {
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
        chat_loop(np).await?;
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

    println!("bootstrapping tor...");
    let tor =
        TorClient::<PreferredRuntime>::create_bootstrapped(TorClientConfig::default()).await?;

    match args[1].as_str() {
        "initiate" => {
            if args.len() < 3 {
                eprintln!("usage: {} initiate <onion_addr>", args[0]);
                std::process::exit(2);
            }
            run_initiator(&tor, &args[2]).await?;
        }
        "listen" => {
            run_responder(&tor).await?;
        }
        _ => {
            eprintln!("unknown mode: {}", args[1]);
            std::process::exit(2);
        }
    }
    Ok(())
}
