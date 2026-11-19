use std::env;
use std::time::Duration;

use asterisk_agi::{Agi, AgiError };

fn main() {
    let address = env::var("FASTAGI_ADDR").unwrap_or_else(|_| "0.0.0.0:4573".to_owned());
    println!("FastAGI server listening on {address}");

    if let Err(error) = Agi::listen(&address, handle_call) {
        eprintln!("FastAGI server stopped: {error}");
        std::process::exit(1);
    }
}

fn handle_call(agi: Agi) {
    if let Err(error) = run_call(&agi) {
        eprintln!("FastAGI call failed: {error}");
    }

    if let Err(error) = agi.close() {
        eprintln!("Failed to close FastAGI connection: {error}");
    }
}

fn run_call(agi: &Agi) -> Result<(), AgiError> {
    let variables = agi.variables.lock().unwrap().clone();
    let caller_id = variables
        .get("agi_callerid")
        .map(String::as_str)
        .unwrap_or("unknown");
    let unique_id = variables
        .get("agi_uniqueid")
        .map(String::as_str)
        .unwrap_or("unknown");

    println!("Call from {caller_id} (unique ID: {unique_id})");

    agi.answer()?;
    agi.verbosef(&format!("Rust FastAGI example handling call from {caller_id}"))?;
    // agi.say_digits("123", "12#")?;
    // agi.say_number("123", "#")?;
     agi.say_phonetic("15202834814", "#")?;

    let music = env::var("FASTAGI_SOUND")
        .unwrap_or_else(|_| "/var/lib/asterisk/sounds/welcome".to_owned());
    let digit = match agi.stream_file(&music, "1234567#", 0) {
        Ok(digit) => {
            println!("digit: {}", digit);
            digit
        }
        Err(err) => {
            println!("err: {}", err);
            String::new()
        }
    };
    // let digit = agi.stream_file(&music, "#", 0)?;

    let digit = if digit.is_empty() {
        agi.wait_for_digit(Duration::from_secs(5))?
    } else {
        digit
    };
    if digit.is_empty() {
        agi.verbose("Caller did not press a key", 1)?;
    } else {
        agi.verbosef(&format!("Caller pressed {digit}"))?;
    }

    agi.hangup()?;
    Ok(())
}
