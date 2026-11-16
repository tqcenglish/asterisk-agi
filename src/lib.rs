//! This crate helps to implement AGI (Asterisk Gateway Interface) programs.
//!
//! Two APIs are provided:
//!
//! * A session-based API centred on [`Agi`], which supports classical AGI over
//!   stdin/stdout, EAGI, and FastAGI over TCP. This is the recommended API.
//! * A function-based API (see [`read_variables`], [`answer`], [`stream_file`],
//!   …) retained for backwards compatibility. It operates on a shared
//!   stdin/stdout session.
//!
//! ```
//! use asterisk_agi::Agi;
//!
//! // In a real program use `Agi::stdio()` (or `Agi::eagi()` / `Agi::conn()`).
//! let input = std::io::Cursor::new(b"agi_test: 1\n\n200 result=1\n".to_vec());
//! let agi = Agi::new(input, std::io::sink());
//! assert!(agi.variables.lock().unwrap().contains_key("agi_test"));
//! ```

mod compat;
mod error;
mod mrcp;
mod response;
mod session;
mod util;

pub use error::{AgiError, State, status};
pub use mrcp::{
    RecognitionInterpretation, RecognitionResult, SynthResult,
};
pub use response::{AgiResult, Response, dtmf_digit, parse_response};
pub use session::{Agi, Handler, RecordOptions};
pub use util::{to_epoch, to_msec, to_sec};

// Function-based API, re-exported at the crate root for backwards compatibility.
pub use compat::{
    MenuItem, answer, exec, get_variable, menu, pause, parse_result, prompt,
    prompt_hour, prompt_phonenumber, read_variables, say_date, say_digits,
    say_number, say_time, set_variable, stream_file, wait_for_digit,
};

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
