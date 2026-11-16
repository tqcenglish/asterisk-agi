//! Session-based AGI client (Stdio, EAGI and FastAGI).

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::io::{FromRawFd, RawFd};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::error::{AgiError, State};
use crate::response::{Response, dtmf_digit, parse_response};
use crate::util::{to_epoch, to_msec, to_sec};

/// Handler invoked for each FastAGI connection.
pub trait Handler: Send + Sync + 'static {
    /// Handle a single AGI session.
    fn handle(&self, agi: Agi);
}

impl<F> Handler for F
where
    F: Fn(Agi) + Send + Sync + 'static,
{
    fn handle(&self, agi: Agi) {
        self(agi)
    }
}

/// A single AGI session.
///
/// The session owns its input/output streams, so it can be backed by stdin/stdout
/// (classical AGI), file descriptors (EAGI) or a TCP socket (FastAGI). A mutex
/// serializes the write-command/read-response round trip, making a session safe
/// to share between threads.
pub struct Agi {
    /// Variables Asterisk supplied at the start of the session.
    pub variables: Mutex<HashMap<String, String>>,
    reader: Mutex<Box<dyn BufRead + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    eagi: Mutex<Option<Box<dyn Read + Send>>>,
    conn: Mutex<Option<TcpStream>>,
    logger: Mutex<Option<Box<dyn Fn(&str) + Send + Sync>>>,
    command_lock: Mutex<()>,
}

impl Agi {
    /// Create a session from arbitrary reader/writer pairs, reading the initial
    /// variables from `reader` until the first blank line.
    pub fn new<R, W>(reader: R, writer: W) -> Self
    where
        R: BufRead + Send + 'static,
        W: Write + Send + 'static,
    {
        Self::with_eagi(reader, writer, None::<Box<dyn Read + Send>>)
    }

    /// Create a session with an additional EAGI audio stream.
    pub fn with_eagi<R, W, E>(reader: R, writer: W, eagi: Option<E>) -> Self
    where
        R: BufRead + Send + 'static,
        W: Write + Send + 'static,
        E: Read + Send + 'static,
    {
        let session = Agi {
            variables: Mutex::new(HashMap::new()),
            reader: Mutex::new(Box::new(reader)),
            writer: Mutex::new(Box::new(writer)),
            eagi: Mutex::new(eagi.map(|e| Box::new(e) as Box<dyn Read + Send>)),
            conn: Mutex::new(None),
            logger: Mutex::new(None),
            command_lock: Mutex::new(()),
        };
        session.read_variables();
        session
    }

    /// Create a session bound to stdin and stdout.
    pub fn stdio() -> Self {
        Self::new(BufReader::new(io::stdin()), io::stdout())
    }

    /// Create an EAGI session: stdin for commands, fd 3 for audio, stdout for replies.
    pub fn eagi() -> Self {
        Self::eagi_fd(3)
    }

    /// Create an EAGI session using a specific file descriptor for the audio stream.
    pub fn eagi_fd(fd: RawFd) -> Self {
        let audio = unsafe { File::from_raw_fd(fd) };
        Self::with_eagi(BufReader::new(io::stdin()), io::stdout(), Some(audio))
    }

    /// Create a FastAGI session bound to an accepted TCP connection.
    pub fn conn(stream: TcpStream) -> io::Result<Self> {
        let reader = stream.try_clone()?;
        let writer = stream.try_clone()?;
        let session = Self::new(BufReader::new(reader), writer);
        *session.conn.lock().unwrap() = Some(stream);
        Ok(session)
    }

    /// Run a FastAGI server, invoking `handler` in a new thread per connection.
    ///
    /// An empty `addr` defaults to `localhost:4573`.
    pub fn listen<H>(addr: &str, handler: H) -> Result<(), AgiError>
    where
        H: Handler,
    {
        let address = if addr.is_empty() { "localhost:4573" } else { addr };
        let listener = TcpListener::bind(address).map_err(|e| AgiError::Bind(e.to_string()))?;
        let handler = Arc::new(handler);

        for incoming in listener.incoming() {
            let stream = incoming.map_err(|e| AgiError::Accept(e.to_string()))?;
            let handler = Arc::clone(&handler);
            std::thread::spawn(move || match Agi::conn(stream) {
                Ok(agi) => handler.handle(agi),
                Err(e) => log::error!("failed to set up FastAGI session: {}", e),
            });
        }
        Ok(())
    }

    /// Close the underlying TCP connection, if any.
    pub fn close(&self) -> io::Result<()> {
        if let Some(conn) = self.conn.lock().unwrap().take() {
            conn.shutdown(Shutdown::Both)?;
        }
        Ok(())
    }

    /// Access the EAGI audio stream, if the session has one.
    pub fn eagi_stream<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut dyn Read) -> R,
    {
        let mut guard = self.eagi.lock().unwrap();
        guard.as_mut().map(|stream| f(stream.as_mut()))
    }

    /// Attach a logger that receives low-level command traces.
    pub fn set_logger<F>(&self, logger: F) -> Result<(), AgiError>
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let mut guard = self.logger.lock().unwrap();
        if guard.is_some() {
            return Err(AgiError::LoggerAlreadyAttached);
        }
        *guard = Some(Box::new(logger));
        drop(guard);

        let variables = self.variables.lock().unwrap().clone();
        for (key, value) in &variables {
            self.log(&format!("${}={}", key, value));
        }
        Ok(())
    }

    fn log(&self, message: &str) {
        if let Some(logger) = self.logger.lock().unwrap().as_ref() {
            logger(message);
        }
    }

    /// Re-read the initial variables from the input stream.
    pub fn read_variables(&self) {
        let mut variables = HashMap::new();
        {
            let mut reader = self.reader.lock().unwrap();
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let line = line.trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    break;
                }
                if let Some((key, value)) = line.split_once(':') {
                    variables.insert(key.trim().to_string(), value.trim().to_string());
                }
            }
        }
        *self.variables.lock().unwrap() = variables;
    }

    /// Send a raw command (as individual arguments) and return the response.
    ///
    /// Arguments containing a newline, carriage return or NUL byte are
    /// rejected before anything is written, closing the AGI command-injection
    /// vulnerability present in naive implementations.
    pub fn command(&self, args: &[&str]) -> Response {
        for (index, arg) in args.iter().enumerate() {
            if arg.contains(['\n', '\r', '\0']) {
                let mut response = Response::default();
                response.error = Some(AgiError::InvalidArgument(format!(
                    "argument {} contains a line break or NUL byte",
                    index
                )));
                return response;
            }
        }

        let command = args.join(" ");
        // Hold a single lock across the whole write/read round trip so that
        // concurrent callers cannot interleave their command and response.
        let _guard = self.command_lock.lock().unwrap();

        let mut writer = self.writer.lock().unwrap();
        if let Err(error) = writeln!(writer, "{}", command).and_then(|_| writer.flush()) {
            let mut response = Response::default();
            response.error = Some(AgiError::Io(error));
            return response;
        }
        drop(writer);

        let mut reader = self.reader.lock().unwrap();
        let mut raw = String::new();
        let response = loop {
            raw.clear();
            match reader.read_line(&mut raw) {
                Ok(0) => {
                    let mut response = Response::default();
                    response.error = Some(AgiError::AgiNoResult);
                    break response;
                }
                Err(error) => {
                    let mut response = Response::default();
                    response.error = Some(AgiError::Io(error));
                    break response;
                }
                Ok(_) => {}
            }

            let line = raw.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                continue;
            }
            if line.starts_with("HANGUP") {
                let mut response = Response::default();
                response.error = Some(AgiError::Hangup);
                break response;
            }
            break parse_response(line).unwrap_or_else(|error| {
                let mut response = Response::default();
                response.error = Some(error);
                response
            });
        };
        drop(reader);

        let detail = match &response.error {
            Some(error) => format!("Err:{}", error),
            None => format!(
                "Sta:{} Res:{} Str:{} Val:{}",
                response.status, response.result, response.result_string, response.value
            ),
        };
        self.log(&format!("#{} -> {} -> {{{}}}", command, raw.trim(), detail));

        response
    }

    // -- Helpers ---------------------------------------------------------

    fn ok(response: Response) -> Result<(), AgiError> {
        match response.error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn into_res(response: Response) -> Result<String, AgiError> {
        match response.error {
            Some(error) => Err(error),
            None => Ok(response.result_string),
        }
    }

    fn into_val(response: Response) -> Result<String, AgiError> {
        match response.error {
            Some(error) => Err(error),
            None => Ok(response.value),
        }
    }

    fn escape_digits(escape: &str) -> &str {
        if escape.is_empty() { "\"\"" } else { escape }
    }

    // -- Core commands ---------------------------------------------------

    /// Answer the channel.
    pub fn answer(&self) -> Result<(), AgiError> {
        Self::ok(self.command(&["ANSWER"]))
    }

    /// Hang up the channel.
    pub fn hangup(&self) -> Result<(), AgiError> {
        Self::ok(self.command(&["HANGUP"]))
    }

    /// Query the channel state.
    pub fn status(&self) -> Result<State, AgiError> {
        let response = self.command(&["CHANNEL STATUS"]);
        let value = Self::into_res(response)?;
        value
            .parse::<i32>()
            .map(State::from_code)
            .map_err(|_| AgiError::Parse(value))
    }

    /// Execute an arbitrary dialplan application, returning its result code.
    pub fn exec(&self, args: &[&str]) -> Result<String, AgiError> {
        let mut parts = Vec::with_capacity(args.len() + 1);
        parts.push("EXEC");
        parts.extend_from_slice(args);
        Self::into_res(self.command(&parts))
    }

    /// Read a channel variable (the `value` field of the response).
    pub fn get(&self, key: &str) -> Result<String, AgiError> {
        Self::into_val(self.command(&["GET VARIABLE", key]))
    }

    /// Set a channel variable.
    pub fn set(&self, key: &str, value: &str) -> Result<(), AgiError> {
        Self::ok(self.command(&["SET VARIABLE", key, value]))
    }

    /// Play a file and collect DTMF digits, returning the raw result string.
    pub fn get_data(
        &self,
        sound: &str,
        timeout: Duration,
        max_digits: u32,
    ) -> Result<String, AgiError> {
        let file = if sound.is_empty() { "silence/1" } else { sound };
        let timeout_ms = to_msec(timeout);
        let max = max_digits.to_string();
        Self::into_res(self.command(&["GET DATA", file, &timeout_ms, &max]))
    }

    /// Play a file, returning the DTMF digit pressed (empty string if none).
    pub fn stream_file(
        &self,
        name: &str,
        escape_digits: &str,
        offset: i32,
    ) -> Result<String, AgiError> {
        let escape = Self::escape_digits(escape_digits);
        let offset_str = offset.to_string();
        dtmf_digit(self.command(&["STREAM FILE", name, escape, &offset_str]))
    }

    /// Wait for a DTMF digit, returning it as a string (empty on timeout).
    pub fn wait_for_digit(&self, timeout: Duration) -> Result<String, AgiError> {
        let timeout_ms = to_msec(timeout);
        dtmf_digit(self.command(&["WAIT FOR DIGIT", &timeout_ms]))
    }

    /// Record audio to a file.
    pub fn record(&self, name: &str, options: &RecordOptions) -> Result<(), AgiError> {
        let timeout_ms = to_msec(options.timeout);
        let mut cmd = format!(
            "RECORD FILE {} {} {} {}",
            name, options.format, options.escape_digits, timeout_ms
        );
        if options.offset > 0 {
            cmd.push_str(&format!(" {}", options.offset));
        }
        if options.beep {
            cmd.push_str(" BEEP");
        }
        if options.silence > Duration::ZERO {
            cmd.push_str(&format!(" s={}", to_sec(options.silence)));
        }
        Self::ok(self.command(&[&cmd]))
    }

    /// Log a message to Asterisk's verbose log.
    pub fn verbose(&self, message: &str, level: i32) -> Result<(), AgiError> {
        let quoted = quote(message);
        let level_str = level.to_string();
        Self::ok(self.command(&["VERBOSE", &quoted, &level_str]))
    }

    /// Log a formatted message at verbose level 9.
    pub fn verbosef(&self, message: &str) -> Result<(), AgiError> {
        self.verbose(message, 9)
    }

    // -- Say commands ----------------------------------------------------

    /// Spell out a label character by character.
    pub fn say_alpha(&self, label: &str, escape_digits: &str) -> Result<String, AgiError> {
        let escape = Self::escape_digits(escape_digits);
        dtmf_digit(self.command(&["SAY ALPHA", label, escape]))
    }

    /// Speak a string of digits.
    pub fn say_digits(&self, digits: &str, escape_digits: &str) -> Result<String, AgiError> {
        let escape = Self::escape_digits(escape_digits);
        dtmf_digit(self.command(&["SAY DIGITS", digits, escape]))
    }

    /// Speak a number.
    pub fn say_number(&self, number: &str, escape_digits: &str) -> Result<String, AgiError> {
        let escape = Self::escape_digits(escape_digits);
        dtmf_digit(self.command(&["SAY NUMBER", number, escape]))
    }

    /// Speak a date.
    pub fn say_date(
        &self,
        when: &chrono::DateTime<chrono_tz::Tz>,
        escape_digits: &str,
    ) -> Result<String, AgiError> {
        let escape = Self::escape_digits(escape_digits);
        let epoch = to_epoch(when);
        dtmf_digit(self.command(&["SAY DATE", &epoch, escape]))
    }

    /// Speak a date/time using an Asterisk format string.
    pub fn say_datetime(
        &self,
        when: &chrono::DateTime<chrono_tz::Tz>,
        escape_digits: &str,
        format: &str,
    ) -> Result<String, AgiError> {
        let escape = Self::escape_digits(escape_digits);
        let fmt = if format.is_empty() {
            "ABdY 'digits/at' IMp"
        } else {
            format
        };
        let zone = when.format("%Z").to_string();
        let epoch = to_epoch(when);
        dtmf_digit(self.command(&["SAY DATETIME", &epoch, escape, fmt, &zone]))
    }

    /// Spell a phrase phonetically.
    pub fn say_phonetic(&self, phrase: &str, escape_digits: &str) -> Result<String, AgiError> {
        let escape = Self::escape_digits(escape_digits);
        dtmf_digit(self.command(&["SAY PHONETIC", phrase, escape]))
    }

    /// Speak the time portion of a timestamp.
    pub fn say_time(
        &self,
        when: &chrono::DateTime<chrono_tz::Tz>,
        escape_digits: &str,
    ) -> Result<String, AgiError> {
        let escape = Self::escape_digits(escape_digits);
        let epoch = to_epoch(when);
        dtmf_digit(self.command(&["SAY TIME", &epoch, escape]))
    }
}

use std::fs::File;

/// Options controlling [`Agi::record`].
#[derive(Debug, Clone)]
pub struct RecordOptions {
    /// Recording format, defaults to `wav`.
    pub format: String,
    /// Digits that terminate the recording, defaults to `#`.
    pub escape_digits: String,
    /// Maximum recording time, defaults to 5 minutes.
    pub timeout: Duration,
    /// Maximum silence allowed before ending, 0 disables it.
    pub silence: Duration,
    /// Whether to play a beep before recording.
    pub beep: bool,
    /// Number of samples to skip before storing.
    pub offset: i32,
}

impl Default for RecordOptions {
    fn default() -> Self {
        RecordOptions {
            format: "wav".to_string(),
            escape_digits: "#".to_string(),
            timeout: Duration::from_secs(5 * 60),
            silence: Duration::ZERO,
            beep: false,
            offset: 0,
        }
    }
}

/// Quote a string the way Go's `strconv.Quote` does, so that messages
/// containing newlines stay a single AGI argument.
fn quote(message: &str) -> String {
    let mut out = String::with_capacity(message.len() + 2);
    out.push('"');
    for ch in message.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\x00"),
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[derive(Clone)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn session(response: &str) -> (Agi, Arc<Mutex<Vec<u8>>>) {
        let input = format!("agi_test: 1\n\n{}", response);
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let agi = Agi::new(Cursor::new(input.into_bytes()), SharedWriter(buffer.clone()));
        (agi, buffer)
    }

    fn written_text(buffer: &Arc<Mutex<Vec<u8>>>) -> String {
        String::from_utf8(buffer.lock().unwrap().clone()).unwrap()
    }

    #[test]
    fn reads_initial_variables() {
        let (agi, _) = session("200 result=1\n");
        let variables = agi.variables.lock().unwrap();
        assert_eq!(variables.get("agi_test").map(String::as_str), Some("1"));
    }

    #[test]
    fn rejects_injection_attempts() {
        for bad in [
            "user\nEXEC System pwn",
            "user\r\nEXEC System pwn",
            "user\0EXEC System pwn",
        ] {
            let (agi, written) = session("200 result=1\n");
            let response = agi.command(&["SET VARIABLE", "NAME", bad]);
            assert!(matches!(response.error, Some(AgiError::InvalidArgument(_))));
            assert_eq!(written_text(&written), "", "nothing should be written");
        }
    }

    #[test]
    fn executes_clean_command() {
        let (agi, written) = session("200 result=1\n");
        assert!(agi.set("NAME", "Voicemail User").is_ok());
        assert_eq!(written_text(&written), "SET VARIABLE NAME Voicemail User\n");
    }

    #[test]
    fn detects_hangup() {
        let (agi, _) = session("HANGUP\n");
        assert!(matches!(agi.answer(), Err(AgiError::Hangup)));
    }

    #[test]
    fn verbose_escapes_line_breaks() {
        let (agi, written) = session("200 result=0\n");
        assert!(agi.verbose("multi\nline message", 1).is_ok());
        assert_eq!(written_text(&written), "VERBOSE \"multi\\nline message\" 1\n");
    }

    #[test]
    fn empty_escape_digits_becomes_empty_quotes() {
        let (agi, written) = session("200 result=54\n");
        assert_eq!(agi.stream_file("demo", "", 0).unwrap(), "6");
        assert_eq!(written_text(&written), "STREAM FILE demo \"\" 0\n");
    }

    #[test]
    fn commands_are_serialized() {
        let (agi, written) = session("200 result=1\n200 result=1\n");
        let agi = Arc::new(agi);
        let a2 = Arc::clone(&agi);
        let h = std::thread::spawn(move || a2.status().unwrap());
        assert_eq!(agi.status().unwrap(), State::Reserved);
        assert_eq!(h.join().unwrap(), State::Reserved);
        assert_eq!(written_text(&written), "CHANNEL STATUS\nCHANNEL STATUS\n");
    }
}
