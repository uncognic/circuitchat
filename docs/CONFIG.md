# Configuration reference

The config file is created automatically next to the binary on first run (`circuitchat.toml`). It is a standard TOML file.

## Default config

```toml
[identity]
persist = false

[history]
save = false
passphrase = ""

[time]
24h = true
local = false
show_tz = false
show_seconds = false

[auth]
enabled = false
password = ""

[privacy]
typing_status = false
read_receipts = false
randomize_filenames = true

[bridge]
enabled = false
lines = []
```

## `[identity]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `persist` | bool | `false` | When `true`, Tor state and cache are saved to `state/` and `cache/` next to the binary, keeping your onion address stable across runs. Required for `history.save`. |

## `[history]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `save` | bool | `false` | Persist messages to an encrypted SQLite database (`circuitchat.db`). Requires `identity.persist = true`. |
| `passphrase` | string | `""` | Passphrase used to encrypt the message database. If empty, you are prompted interactively at startup. On first run you will be asked to confirm the passphrase. |

> **Note:** setting `history.save = true` without `identity.persist = true` has no effect and will print a warning at startup.


## `[time]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `24h` | bool | `true` | Display timestamps in 24-hour format. Set `false` for 12-hour (AM/PM). |
| `local` | bool | `false` | Use local system time for timestamps. When `false`, UTC is used. |
| `show_tz` | bool | `false` | Append the timezone abbreviation to timestamps. |
| `show_seconds` | bool | `false` | Include seconds in timestamps. |


## `[auth]`

Optional shared-password authentication. When enabled, the listener requires the initiator to prove knowledge of the password before the chat session starts. Both sides authenticate each other (mutual). See [Security: Authentication](security.md#authentication)

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Require password authentication on every connection. |
| `password` | string | `""` | The session password. If empty, you are prompted interactively at startup. |


## `[privacy]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `typing_status` | bool | `false` | Send typing start/stop notifications to your peer when you begin or clear the input field. |
| `read_receipts` | bool | `false` | Send a "delivered" acknowledgement when a message is received. The sender's TUI marks the message as delivered. |
| `randomize_filenames` | bool | `true` | When sending a file, randomize the filename to avoid revealing information about the file's original name. |

Both features are opt-in and only active when both sides have them enabled in their own configs. A peer that does not have `typing_status` enabled will simply ignore the control messages.


## `[bridge]`

Tor bridges allow circuitchat to work in networks that censor or block direct Tor connections. See [Usage - bridges](usage.md#bridges) for usage guidance.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Use the configured bridge lines when bootstrapping Tor. |
| `lines` | array of strings | `[]` | One or more bridge lines. Each entry is a quoted string. |

### Example

```toml
[bridge]
enabled = true
lines = [
    "82.69.107.17:9001 42B0A9BA84007D81B329F2ECB86D2F44D3CA995C",
    "194.36.145.3:9001 2DC7C3A77E2EF0A15D16EBCA0050B73DC91E7C27"
]
```

Bridge lines can be obtained from [bridges.torproject.org](https://bridges.torproject.org/)


## Resetting saved state

The `--reset` flag deletes the database, Tor cache, and Tor state directories, effectively giving you a fresh identity: `circuitchat --reset`

The config file (`circuitchat.toml`) is not deleted by `--reset`.