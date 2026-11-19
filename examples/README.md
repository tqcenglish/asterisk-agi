# FastAGI examples

Both examples start a FastAGI server on `0.0.0.0:4573`. Set `FASTAGI_ADDR` to
change the bind address. The default listens on all network interfaces, so allow
inbound connections to the selected port in the host and network firewalls.

## General demo

`demo/main.rs` answers a call, speaks a few digit examples, plays one sound file, waits
up to five seconds for a DTMF key if none was pressed during playback, and hangs
up.

```sh
FASTAGI_ADDR=127.0.0.1:4573 FASTAGI_SOUND=custom/welcome \
  cargo run --example demo
```

`FASTAGI_SOUND` is a sound name relative to Asterisk's sounds directory, without
the extension. For example, `custom/welcome` can refer to
`welcome.wav` in `sounds/custom/`.

## Rust AGI server and request routing

`rust_agiserver/main.rs` reads the FastAGI `agi_request` variable and dispatches
requests by URI path. `/play_worknumber` calls
`rust_agiserver/play_worknumber/main.rs`; other paths are reported as not
implemented to the server log and Asterisk verbose log.

Start in the foreground:

```sh
FASTAGI_ADDR=127.0.0.1:4573 cargo run --example rust_agiserver -- run
```

For background operation and Nginx-style controls, use:

```sh
cargo run --example rust_agiserver -- daemon
cargo run --example rust_agiserver -- status
cargo run --example rust_agiserver -- reload
cargo run --example rust_agiserver -- stop
```

`reload` sends SIGHUP. A valid new config replaces the current one for new calls;
if parsing fails, the server logs the error and keeps the previous config. PID and
log files default to `/tmp/rust_agiserver.pid` and `/tmp/rust_agiserver.log`.
Override them with `RUST_AGISERVER_PID_FILE` and `RUST_AGISERVER_LOG_FILE`.

The handler loads the three `play_worknumber` settings from
`examples/rust_agiserver/config.yaml`, relative to the project root by default.
Set `RUST_AGISERVER_CONFIG` to override that path.

`MEMBERNAME` is treated as the raw extension string, for example `1001`. The
handler compares it exactly against the values in `worknumber_extension_map`.
When it finds a match, it plays the configured before audio, speaks the matching
work number with `SAY DIGITS`, then plays the after audio. It does not dial the
extension and does not add a `PJSIP/` prefix.

```yaml
play_worknumber:
  before_worknumber_audio_path: "custom/before_worknumber"
  after_worknumber_audio_path: "custom/after_worknumber"
  worknumber_extension_map:
    "12345": "1001"
    "67890": "1002"
```

Audio names are relative to Asterisk's sounds directory and omit the extension.
Install supported audio files such as `before_worknumber.wav` and
`after_worknumber.wav` under `sounds/custom/`.

Set `MEMBERNAME` in the Asterisk dialplan before invoking FastAGI:

```ini
[from-internal]
exten => 700,1,Set(MEMBERNAME=1001)
 same => n,AGI(agi://127.0.0.1:4573/play_worknumber)
 same => n,Hangup()
```

## Build the ARM64 Linux binary

From the project root, run the Makefile target:

```sh
make rust_agiserver
```

This builds with `CARGO_TARGET_DIR=./target` and embeds the UTC build date and
short Git commit hash. The binary is written to
`target/aarch64-unknown-linux-gnu/release/examples/rust_agiserver`. On the
ARM64 Linux host, inspect its build metadata with:

```sh
./target/aarch64-unknown-linux-gnu/release/examples/rust_agiserver -v
```

The version option prints the metadata and exits without starting the server.

## Configure Asterisk

Replace `127.0.0.1` in the dialplan with the FastAGI server's reachable IP
when Asterisk runs on a different host. Start one example server, then dial
extension `700`. The server routes by the final path component of `agi_request`.
