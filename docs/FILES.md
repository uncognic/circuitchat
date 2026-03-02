# File transfer
## Overview
circuitchat supports sending and receiving files within a chat session. All file data is sent through the same Noise-encrypted Tor stream as regular messages - there is no separate connection.

## Sending a file
`/send /absolute/or/relative/path/to/file.zip`. This sends a file offer to your peer. The offer includes the filename and file size. The actual transfer does not start until the peer accepts.

While waiting for the peer to respond, the chat session continues normally. Only one outgoing offer can be pending at a time.

## Receiving a file
When a peer offers a file, a message appears in the chat:

```
[file] peer wants to send report.pdf (2.3 MB) - type /accept or /reject
```

Accepted files are saved to the `downloads/` folder next to the binary. If a file with the same name already exists, a suffix is appended (`report_1.pdf`).

## Filename sanitisation
Received filenames are sanitised before saving:
- The following characters are replaced with `_`: `/ \ : * ? " < > |`

This prevents path traversal attacks.

## Limitations
- Only one file transfer can be active at a time
- There is no resume support. If a transfer is cancelled it must be restarted from scratch
