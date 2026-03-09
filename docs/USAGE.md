# Getting started
## Requirements
- Rust (if compiling from source)
## Installation
### From source

```sh
git clone https://github.com/your-username/circuitchat
cd circuitchat
cargo build --release
```
Note: you might run into issues with OpenSSL and RuSQLite on some platforms. Install the appropriate development packages for your OS (e.g. `libssl-dev` and `libsqlite3-dev` on Debian/Ubuntu).

The compiled binary is at `target/release/circuitchat` (or `circuitchat.exe` on Windows). 

### Precompiled binaries
Precompiled binaries for major platforms are available in the releases section. 

### Files created on first run
All runtime files are placed in the same directory as the binary:

| File / folder | Purpose |
|---------------|---------|
| `circuitchat.toml` | Config file |
| `circuitchat.db` | Encrypted message history (only when `history.save = true`) |
| `cache/` | Tor directory cache (only when `identity.persist = true`) |
| `state/` | Tor state, including your onion service key (only when `identity.persist = true`) |
| `downloads/` | Files received from peers |

## First run
### Listener side

```sh
./circuitchat listen
```

1. Tor bootstraps
2. An onion service is created and the `.onion` address is printed
3. The descriptor is published to the Tor network (typically 10–30 seconds, but can be longer. Try restarting if it takes more than a few minutes)
4. circuitchat waits for your peer to connect

Share the printed `.onion` address with your peer

### Initiator side

```sh
./circuitchat initiate <onion_address>
```

The initiator connects to the given `.onion` address. If the listener's descriptor has not finished publishing yet, the initiator retries every 10 seconds automatically.

Once connected, both sides perform a Noise handshake and (optionally) authenticate. The chat TUI then opens.

## CLI reference

| Command | Description |
|---------|-------------|
| `listen` | Bootstrap Tor, create an onion service, and wait for a peer to connect |
| `initiate <onion_address>` | Bootstrap Tor and connect to the given `.onion` address |
| `bot <script>` | Run a bot script (see [docs/ccscript.md](docs/CCSCRIPT.md)) |
| `--reset` | Delete saved state (`circuitchat.db`, `cache/`, `state/`) and exit |
| `--version` | Print version and exit |

## In-chat commands
| Command | Description |
|---------|-------------|
| `/send <path>` | Offer a file to your peer. `<path>` is the absolute or relative path to the file. |
| `/accept` | Accept a pending incoming file offer |
| `/reject` | Reject a pending incoming file offer |
| `/cancel` | Cancel the active incoming file transfer and delete the partial file |
| `/help` | Show the list of commands |
| `/status` | Show connection and session status |
| `/ping` | Send a ping message to the peer |
| `/panic` / `/wipe` | End the session immediately and delete all state (including config) |
| `/find <query>` | Search message history for `<query>` and show matching messages |
| `/clear` | Clear the screen (does not delete history) |

See [File transfer](file-transfer.md)

## Typing status and delivery receipts

When both sides have `privacy.typing_status = true`, a "peer is typing..." indicator appears in the status bar. It is triggered when the peer starts typing and cleared when they send or erase their message.

When both sides have `privacy.read_receipts = true`, a `✓` marker is appended to a sent message once the peer's client acknowledges receipt. This confirms delivery to the peer's process — not that they have read it.


## Stable identity
By default, a new onion address is generated every time `listen` is run. To keep a stable address across runs:
```toml
# circuitchat.toml
[identity]
persist = true
```
The Tor state (including your onion service private key) is then saved to the `state/` directory. Your address will remain the same until you run `--reset`.

## Bridges
If Tor is blocked on your network, configure bridges:
```toml
[bridge]
enabled = true
lines = [
    "0.0.0.0:123 FINGERPRINT_HERE"
]
```
Obtain bridge lines from [bridges.torproject.org](https://bridges.torproject.org/)

## Resetting state
To delete the saved identity, Tor cache, and message database (but keep your config):

```sh
./circuitchat --reset
```

## Platform notes
Windows: Windows Terminal is recommended.
