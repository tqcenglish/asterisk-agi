# rust_agiserver architecture

## Files

- `main.rs` handles `daemon`, `reload`, `stop`, and `status` commands, loads configuration, manages the FastAGI listener, and dispatches `agi_request` by URI path.
- `play_worknumber/mod.rs` exposes the request handler module.
- `play_worknumber/main.rs` reads `MEMBERNAME`, finds the matching extension value in the worknumber map, and plays the configured audio and worknumber.
- `config.yaml` contains audio paths and the worknumber-to-extension map.

## Request flow

1. Asterisk connects to the FastAGI listener and sends its initial channel variables.
2. The server extracts the final path component from `agi_request`.
3. `/play_worknumber` invokes the `play_worknumber` handler. Other paths are logged as not implemented.
4. The handler reads `MEMBERNAME` as a raw extension value, such as `1001`.
5. It finds the worknumber whose map value exactly equals `MEMBERNAME`.
6. It plays the before audio, speaks the worknumber with `SAY DIGITS`, and plays the after audio.

The handler does not dial the extension or prepend `PJSIP/`.

## Daemon and reload

Run these commands from the project root, or invoke the built binary with the same subcommand:

```sh
cargo run --example rust_agiserver -- daemon
cargo run --example rust_agiserver -- status
cargo run --example rust_agiserver -- reload
cargo run --example rust_agiserver -- stop
```

`daemon` starts a detached process and writes its PID to `/tmp/rust_agiserver.pid`; standard output and errors append to `/tmp/rust_agiserver.log`. `reload` sends SIGHUP. The listener keeps serving while the config is parsed; successful reloads apply to new connections, and failed reloads leave the last valid config active. `stop` sends SIGTERM. Set `RUST_AGISERVER_PID_FILE` and `RUST_AGISERVER_LOG_FILE` to override the paths. Set the same PID file environment variable when issuing control commands if it was overridden.

## Configuration

By default, `rust_agiserver` reads `examples/rust_agiserver/config.yaml` relative to the process working directory. Set `RUST_AGISERVER_CONFIG` to use a different path.

```yaml
play_worknumber:
  before_worknumber_audio_path: "custom/before_worknumber"
  after_worknumber_audio_path: "custom/after_worknumber"
  worknumber_extension_map:
    "12345": "1001"
    "67890": "1002"
```

The map key is the worknumber and the value is the extension text compared with `MEMBERNAME`. Audio paths are relative to Asterisk's sounds directory and omit the file extension.
