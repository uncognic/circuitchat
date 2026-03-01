# Configuration
A `circuitchat.toml` file is created next to the binary on first run:

```toml
[identity]
persist = false

[history]
save = false
passphrase = ""

[time]
24h = false
local = false
show_tz = false
show_seconds = false

[auth]
enabled = false
password = ""

[privacy]
typing_status = false
read_receipts = false

```
`identity.persist`: When `true`, Tor state and cache are saved between runs so the onion address remains stable, and chat history is stored locally if history.save = true.\
`history.save`: When `true` (requires `identity.persist = true`), messages are saved to an encrypted SQLite database.\
`history.passphrase`: Hardcoded passphrase for the message database. If left empty, the passphrase is prompted interactively at startup.\
`time.24h`: When `true`, timestamps are displayed in 24-hour format.\
`time.local`: When `true`, timestamps are displayed in local time with timezone. Otherwise, UTC time is used.\
`time.show_tz`: When `true`, timestamps include the timezone.\
`time.show_seconds`: When `true`, timestamps include seconds.\
`auth.enabled`: When `true`, clients must provide a password to connect.\
`auth.password`: Hardcoded password for client authentication. If left empty, the password is prompted interactively at startup.\
`privacy.typing_status`: When `true`, typing status messages are sent to the peer.\
`privacy.read_receipts`: When `true`, "delivered" status messages are sent when messages are received.
