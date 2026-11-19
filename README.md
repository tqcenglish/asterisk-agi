# asterisk-agi

A minimal crate for writing Asterisk AGI (Asterisk Gateway Interface) programs in Rust.


## Two APIs

### Session API (recommended)

`Agi` owns its streams, so the same code works for classical AGI (stdin/stdout),
EAGI (fd 3 audio) and FastAGI (TCP):

```rust,ignore
use asterisk_agi::Agi;
use std::time::Duration;

// Classical AGI
let agi = Agi::stdio();
agi.answer()?;
agi.stream_file("welcome", "#", 0)?;
let digit = agi.wait_for_digit(Duration::from_secs(5))?;
agi.hangup()?;

// FastAGI: one thread per connection
Agi::listen("localhost:4573", |agi| {
    let _ = agi.answer();
})?;
```

Sessions also expose `get`/`set`, `get_data`, `record`, the full `say_*` family,
`exec`, `verbose`, channel `status`, logging via `set_logger`, and the MRCP
helpers (`mrcp_synth`, `mrcp_recog`, `synth_and_recog`, …).

### Function API (backwards compatible)

The original free functions (`read_variables`, `answer`, `stream_file`, `menu`,
`prompt`, `say_date`, …) are still re-exported at the crate root. They share a
single stdin/stdout session internally.

## Safety

Command arguments containing a newline, carriage return or NUL byte are rejected
before being written to the AGI transport, closing the AGI command-injection
vector (e.g. `EXEC System <cmd>`). HANGUP responses are surfaced as
`AgiError::Hangup`, and DTMF digits are read from `result=` rather than the
trailing value field.
