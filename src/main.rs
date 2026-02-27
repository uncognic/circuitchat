use std::env;
use std::error::Error;
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

mod noise_client;
use noise_client::NoiseClient;

fn run_client(addr: &str, pattern: &str) -> Result<(), Box<dyn Error>> {
    let stream = TcpStream::connect(addr)?;
    let mut nc = NoiseClient::connect(stream, pattern)?;
    nc.send(b"hello from client")?;
    let reply = nc.recv()?;
    println!("server replied: {}", String::from_utf8_lossy(&reply));
    Ok(())
}

fn run_server(addr: &str, pattern: &str) -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(addr)?;
    println!("listening on {}", addr);
    for stream in listener.incoming() {
        let stream = stream?;
        let mut nc = NoiseClient::accept(stream, pattern)?;
        let msg = nc.recv()?;
        println!("got: {}", String::from_utf8_lossy(&msg));
        nc.send(b"hello from server")?;
        break;
    }
    Ok(())
}

fn run_p2p(local_addr: &str, peer_addr: &str, pattern: &str) -> Result<(), Box<dyn Error>> {
    if local_addr < peer_addr {
        let start = Instant::now();
        let mut stream: Option<TcpStream> = None;
        while start.elapsed() < Duration::from_secs(5) {
            match TcpStream::connect(peer_addr) {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(_) => thread::sleep(Duration::from_millis(100)),
            }
        }
        let stream = stream.ok_or(Box::<dyn std::error::Error>::from(
            "failed to connect to peer",
        ))?;
        let mut nc = match NoiseClient::connect(stream, pattern) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("p2p initiator handshake failed: {}", e);
                return Err(e);
            }
        };
        nc.send(b"hello from p2p-initiator")?;
        let reply = nc.recv()?;
        println!("peer replied: {}", String::from_utf8_lossy(&reply));
    } else {
        println!("waiting for incoming connection on {}", local_addr);
        let (stream, _) = TcpListener::bind(local_addr)?.accept()?;
        let mut nc = match NoiseClient::accept(stream, pattern) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("p2p responder handshake failed: {}", e);
                return Err(e);
            }
        };
        let msg = nc.recv()?;
        println!("got: {}", String::from_utf8_lossy(&msg));
        nc.send(b"hello from p2p-responder")?;
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "usage: {} (client|server|p2p) addr [peer_addr_for_p2p]",
            args[0]
        );
        std::process::exit(2);
    }
    let mode = &args[1];
    let addr = &args[2];
    static PATTERN: &str = "Noise_NN_25519_ChaChaPoly_BLAKE2s";

    match mode.as_str() {
        "client" => run_client(addr, PATTERN)?,
        "server" => run_server(addr, PATTERN)?,
        "p2p" => {
            if args.len() < 4 {
                eprintln!("p2p mode requires local_addr peer_addr");
                std::process::exit(2);
            }
            let peer = &args[3];
            run_p2p(addr, peer, PATTERN)?;
        }
        _ => {
            eprintln!("unknown mode: {}", mode);
            std::process::exit(2);
        }
    }
    Ok(())
}
