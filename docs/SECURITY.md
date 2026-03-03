# Security
## Threat model
circuitchat is designed to protect against:
- Eavesdroppers on the network: all traffic is encrypted end-to-end with Noise and additionally protected by Tor's transport encryption
- Traffic analysis: routing through Tor hides both parties IP addresses
- Encryption: ephemeral Noise keys provide forward secrecy; capturing today's ciphertext is useless after the session ends
- Identity linkage: by default every session uses a fresh onion address, so there is no persistent identifier

circuitchat does not protect against:
- Compromised peer: if your machine is compromised, an attacker can read your messages directly

## Noise protocol
Every session uses: the `Noise_NN_25519_ChaChaPoly_BLAKE2s` pattern.

The handshake consists of two messages (one from initiator, one from responder) after which both sides derive a shared transport key. All subsequent messages are encrypted with this key.

Because `NN` uses no static keys, there is no key authentication by design, in order to keep identity temporary. Password authentication is available as a mitigation.

## Authentication
The optional password authentication layer runs after the Noise handshake completes. It is a mutual challenge-response protocol:

1. The listener sends a single flag byte (`0x01` = auth required, `0x00` = no auth)
2. If auth is required, the initiator sends a proof: `HMAC-SHA256(key, "circuitchat-auth-initiator")`
3. The listener verifies the proof. If it fails, it sends `0xFF` and closes the connection
4. The listener sends its own proof: `HMAC-SHA256(key, "circuitchat-auth-responder")`
5. The initiator verifies the listener's proof

There is also a fingerprint which can be used to verify that the connection has not been intercepted.
## Local storage
When `history.save = true`, messages are stored in `circuitchat.db` using XChaCha20-Poly1305 encryption per message, with a key derived via Argon2 from your passphrase.

## Anonymity notes
- When `identity.persist = false` (default), a new ephemeral onion address is generated each run. There is no persistent identifier
- When `identity.persist = true`, your onion address is stable. You should treat it as a pseudonym and be aware that reusing an address over time allows an observer to link sessions
