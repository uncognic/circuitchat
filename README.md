# circuitchat
P2P encrypted messaging over Tor made in Rust, with support for file transfer.

It creates ephemeral Tor onion services for real-time messaging with the [Noise Protocol Framework](https://noiseprotocol.org/). There is no server and no identity, all connections are ephemeral. It is meant to reduce metadata leakage.
![GIF of circuitchat](metadata/gif.gif)
## Features

- Tor-first: Uses [Arti](https://gitlab.torproject.org/tpo/core/arti) to bootstrap a Tor client and expose onion services directly from the binary. No external Tor daemon required.
- E2EE: Every session performs a full Noise_NN handshake (`Noise_NN_25519_ChaChaPoly_BLAKE2s`) over the Tor stream, so traffic is encrypted independent of Tor's own transport layer.
- Forward secrecy: The ephemeral Noise handshakes provide forward secrecy, so compromise of stored data does not expose past session messages.
- TUI: Chat interface powered by [ratatui](https://github.com/ratatui/ratatui) with scrollable message history, timestamps, and a status bar.
- Encrypted local history: Optionally persist chat messages in a local SQLite database, encrypted at rest with XChaCha20-Poly1305 (key derived from a passphrase via Argon2).
- Zero-config defaults: Works out of the box with a single command, no setup required (other than installing Rust)

## Usage
### Listen for a connection
```sh
circuitchat listen
```
The program bootstraps Tor, creates an onion service, and prints your `.onion` address. Share this address with your peer.

### Connect to a peer
```sh
circuitchat initiate <onion_address>
```
Connects to the given onion address over Tor and starts a chat session.

## Configuration
See [CONFIG.md](docs/CONFIG.md)

## Disclaimer
The `Noise_NN` pattern provides no authentication, it protects against passive eavesdroppers but not active MITM attacks beyond what Tor itself provides. This is mitigated by allowing users to set a password for authentication.
