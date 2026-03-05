# circuitchat bot scripting language - spec
file extension: .ccscript

## Invocation
`circuitchat bot myscript.ccscript` runs circuitchat in headless bot mode, no TUI.

## Structure

a script is a list of handlers. each handler listens for one event, optionally checks conditions, and runs actions. 

```
on <event>
    [if <condition>]
        <action>
        <action>
    [end]
    [if <condition>]
        ...
    [end]
    [<action>]
    [<action>]
end
```

actions outside any if block run unconditionally whenever the event fires.

## Events
these keywords must be preceded by `on` to start a handler block:

- `connect`: fires when a peer connects (after handshake + version negotiate)
- `disconnect`: fires on peer disconnect
- `message`: any text message received
- `file`: peer offers a file

## Conditions

conditions only appear inside if/end blocks. they operate on the current event's context.

### message conditions
- `if contains "text"`: message contains substring
- `if starts_with "text"`: message starts with string
- `if ends_with "text"`: message ends with string
- `if equals "text"`: message exactly equals string

### message length conditions
- `if message_length > 100`: message length in characters, greater than
- `if message_length < 10`: message length, less than
- `if message_length == 5`: message length, exact match

### file conditions (on file event)
- `if file_size > 1048576`: file size in bytes
- `if file_size < 512`: file size less than
- `if file_name ends_with ".exe"`: file name ends with
- `if file_name starts_with "report"`: file name starts with
- `if file_name contains "draft"`: file name contains
- `if file_name equals "readme.txt"`: file name exact match

### negation
- `if not contains "text"`: negate any condition
- `if not file_name ends_with ".exe"`: negate file_name condition
- `if not message_length > 500`: negate length condition

`not` is a prefix modifier on any condition, not a standalone keyword.

## Actions

- `reply "text"`: send a message to peer
- `log "text"`: print to local stdout, not sent
- `accept`: accept incoming file offer (file event only)
- `reject`: reject incoming file offer (file event only)
- `disconnect`: close the session
- `wait <ms>`: pause for N milliseconds before continuing

## Variables

variables are expanded inside quoted strings with `${}`:

### message variables
- `${message}`: full text of received message
- `${message_length}`: character count of received message
- `${message_upper}`: message converted to uppercase
- `${message_lower}`: message converted to lowercase
- `${message_trimmed}`: message with leading/trailing whitespace removed
- `${message_words}`: word count of message
- `${message_reversed}`: message with characters reversed

### file variables
- `${file_name}`: name of offered file
- `${file_size}`: size of offered file in bytes
- `${file_size_fmt}`: human-readable file size (e.g. "2.0 MB")
- `${file_ext}`: file extension including dot (e.g. ".pdf")

### time & date variables
- `${time}`: current local time, 24h format (HH:MM:SS)
- `${time12}`: current local time, 12h format (hh:MM:SS AM/PM)
- `${date}`: current date (YYYY-MM-DD)
- `${datetime}`: date and time (YYYY-MM-DD HH:MM:SS)
- `${iso8601}`: ISO 8601 timestamp with timezone offset
- `${timestamp}`: unix epoch in seconds
- `${unix}`: alias for timestamp
- `${year}`: four-digit year
- `${month}`: two-digit month (01-12)
- `${day}`: two-digit day (01-31)
- `${hour}`: two-digit hour, 24h (00-23)
- `${minute}`: two-digit minute (00-59)
- `${second}`: two-digit second (00-59)
- `${weekday}`: full weekday name (e.g. "Tuesday")

### session & bot variables
- `${fingerprint}`: noise session fingerprint for the current connection
- `${uptime}`: how long the bot has been running (e.g. "2h 15m 3s")
- `${uptime_secs}`: uptime in raw seconds
- `${connections}`: total number of peer connections since bot started
- `${version}`: circuitchat version (e.g. "1.7.6")

### random & utility variables
- `${random}`: random integer 0-99
- `${random1000}`: random integer 0-999
- `${uuid}`: pseudo-random UUID string

### examples
```
reply "you said: ${message}"
reply "current time: ${time}"
reply "bot uptime: ${uptime} | connections: ${connections}"
log "[${datetime}] received: ${message}"
reply "file ${file_name} is ${file_size_fmt} (ext: ${file_ext})"
```

## Comments
`// this is a comment`

any line starting with // (after stripping whitespace) is ignored.

## Full Example
```
// greet on connect
on connect
    reply "hello, bot connected"
    reply "running circuitchat v${version}"
end

// respond to commands
on message
    if starts_with "!help"
        reply "commands: !help, !time, !echo, !info, !uptime"
    end

    if starts_with "!time"
        reply "current time: ${time} (${weekday}, ${date})"
    end

    if starts_with "!echo "
        reply "${message}"
    end

    if starts_with "!info"
        reply "fingerprint: ${fingerprint}"
        reply "connections: ${connections} | uptime: ${uptime}"
        reply "your message was ${message_length} chars, ${message_words} words"
    end

    if starts_with "!uptime"
        reply "bot has been running for ${uptime} (${uptime_secs}s)"
    end

    if equals "ping"
        reply "pong"
    end

    if message_length > 500
        reply "that's a long message (${message_length} chars)"
    end
end

// auto-reject executables, accept everything else
on file
    if file_name ends_with ".exe"
        reject
        log "[${datetime}] rejected exe: ${file_name}"
    end
    if not file_name ends_with ".exe"
        accept
        log "[${datetime}] accepted file: ${file_name} (${file_size_fmt})"
    end
end

// log disconnects
on disconnect
    log "[${datetime}] peer disconnected (session fingerprint: ${fingerprint})"
end
```
