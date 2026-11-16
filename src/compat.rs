//! Function-based API kept for backwards compatibility.
//!
//! These free functions operate on stdin/stdout through a lazily created
//! [`Agi`](crate::Agi) session. New code should prefer the session API.

use std::collections::HashMap;
use std::io;
use std::sync::Mutex;
use std::{thread, time};

use chrono::{DateTime, NaiveTime, Timelike};
use chrono_tz::Tz;
use lazy_static::lazy_static;

use crate::error::AgiError;
use crate::response::{AgiResult, parse_legacy_result};
use crate::session::Agi;

lazy_static! {
    static ref STDIO: Mutex<Agi> = Mutex::new(Agi::stdio());
}

/// Pause further execution for the given number of seconds.
pub fn pause(seconds: u64) {
    thread::sleep(time::Duration::from_secs(seconds));
}

/// Read the variables given by Asterisk when it calls an AGI program.
pub fn read_variables() -> io::Result<HashMap<String, String>> {
    let agi = STDIO.lock().unwrap();
    agi.read_variables();
    let variables = agi.variables.lock().unwrap().clone();
    Ok(variables)
}

/// Get the content of an Asterisk variable.
pub fn get_variable(name: &str) -> Result<Option<String>, AgiError> {
    let agi = STDIO.lock().unwrap();
    Ok(Some(agi.get(name)?))
}

/// Set an Asterisk variable.
pub fn set_variable(name: &str, value: &str) -> Result<(), AgiError> {
    STDIO.lock().unwrap().set(name, value)
}

/// Parse the result of a command sent to Asterisk (legacy helper).
pub fn parse_result(res: &str) -> Result<AgiResult, AgiError> {
    parse_legacy_result(res)
}

/// Answer a call.
pub fn answer() -> Result<(), AgiError> {
    let result = STDIO.lock().unwrap().answer();
    if result.is_ok() {
        pause(2);
    }
    result
}

/// Play an audio file to the caller.
pub fn stream_file(file: &str, escape_digits: &str) -> Result<Option<char>, AgiError> {
    let digit = STDIO.lock().unwrap().stream_file(file, escape_digits, 0)?;
    Ok(digit.chars().next())
}

/// Wait for the caller to press a key.
pub fn wait_for_digit(timeout_ms: i32) -> Result<Option<char>, AgiError> {
    let digit = STDIO
        .lock()
        .unwrap()
        .wait_for_digit(time::Duration::from_millis(timeout_ms.max(0) as u64))?;
    Ok(digit.chars().next())
}

/// Play an audio file and wait for digits to be pressed by the caller.
pub fn prompt(
    msg_file: &str,
    answer_digits: &str,
    timeout_ms: i32,
) -> Result<Option<char>, AgiError> {
    if let Some(digit) = stream_file(msg_file, answer_digits)? {
        return Ok(Some(digit));
    }
    Ok(wait_for_digit(timeout_ms)?.filter(|key| answer_digits.contains(*key)))
}

/// Play an audio file and wait for a phone number to be entered.
pub fn prompt_phonenumber(
    msg_file: &str,
    timeout_ms: i32,
    digit_timeout_ms: i32,
) -> Result<Option<String>, AgiError> {
    let mut number = String::new();
    match prompt(msg_file, "1234567890", timeout_ms)? {
        Some(digit) => number.push(digit),
        None => return Ok(None),
    }
    loop {
        match wait_for_digit(digit_timeout_ms)? {
            None | Some('#') => break,
            Some(digit) => number.push(digit),
        }
    }
    Ok(Some(number))
}

/// An item presented to the caller in a voice menu.
#[derive(Debug)]
pub enum MenuItem {
    /// Play an audio file.
    File(String),
    /// Speak a sequence of digits.
    Digits(String),
    /// Speak a number.
    Number(i32),
    /// Speak a date.
    Date(DateTime<Tz>),
    /// Speak a time.
    Time(NaiveTime),
}

/// Present a voice menu to the caller.
pub fn menu(
    messages: &[MenuItem],
    answer_digits: &str,
    timeout_ms: i32,
) -> Result<Option<char>, AgiError> {
    for message in messages {
        let agi = STDIO.lock().unwrap();
        let result: String = match message {
            MenuItem::File(file) => agi.stream_file(file, answer_digits, 0)?,
            MenuItem::Digits(digits) => agi.say_digits(digits, answer_digits)?,
            MenuItem::Number(number) => agi.say_number(&number.to_string(), answer_digits)?,
            MenuItem::Date(date) => agi.say_date(date, answer_digits)?,
            MenuItem::Time(t) => menu_say_time(&agi, t, answer_digits)?,
        };
        drop(agi);
        if let Some(digit) = result.chars().next() {
            log::info!("Menu Result: {}", digit);
            return Ok(Some(digit));
        }
    }
    if timeout_ms != 0 {
        Ok(wait_for_digit(timeout_ms)?.filter(|key| answer_digits.contains(*key)))
    } else {
        Ok(None)
    }
}

/// Get the optional second digit of the hour.
fn get_second_hour_digit(first_digit: u32) -> Result<Option<u32>, AgiError> {
    let digit = wait_for_digit(5000)?
        .filter(|ch| ch.is_ascii_digit())
        .map(|ch| (ch as u8 - b'0') as u32);
    Ok(digit.map(|d| first_digit * 10 + d).filter(|h| *h < 24))
}

/// Play a sound file and wait for an hour to be chosen.
pub fn prompt_hour(msg_file: &str) -> Result<Option<u32>, AgiError> {
    match prompt(msg_file, "1234567890", 5000)? {
        Some('0') => get_second_hour_digit(0),
        Some('1') => get_second_hour_digit(1),
        Some('2') => get_second_hour_digit(2),
        Some(d @ '3'..='9') => Ok(Some((d as u8 - b'0') as u32)),
        _ => Ok(None),
    }
}

/// Speak the individual digits of a number.
pub fn say_digits(digits: &str, escape_digits: &str) -> Result<Option<char>, AgiError> {
    Ok(STDIO
        .lock()
        .unwrap()
        .say_digits(digits, escape_digits)?
        .chars()
        .next())
}

/// Speak a number.
pub fn say_number(number: i32, escape_digits: &str) -> Result<Option<char>, AgiError> {
    Ok(STDIO
        .lock()
        .unwrap()
        .say_number(&number.to_string(), escape_digits)?
        .chars()
        .next())
}

/// Speak a date.
pub fn say_date(date: &DateTime<Tz>, escape_digits: &str) -> Result<Option<char>, AgiError> {
    Ok(STDIO
        .lock()
        .unwrap()
        .say_date(date, escape_digits)?
        .chars()
        .next())
}

/// Speak a time inside a voice menu; returns the pressed digit or an empty string.
fn menu_say_time(agi: &Agi, time: &NaiveTime, escape_digits: &str) -> Result<String, AgiError> {
    let hour = agi.say_number(&time.hour().to_string(), escape_digits)?;
    if !hour.is_empty() {
        return Ok(hour);
    }
    let oclock = agi.stream_file("digits/oclock", escape_digits, 0)?;
    if !oclock.is_empty() {
        return Ok(oclock);
    }
    if time.minute() == 0 {
        Ok(String::new())
    } else {
        agi.say_number(&time.minute().to_string(), escape_digits)
    }
}

/// Speak a time.
pub fn say_time(time: &NaiveTime, escape_digits: &str) -> Result<Option<char>, AgiError> {
    let digit = say_number(time.hour() as i32, escape_digits)?
        .or(stream_file("digits/oclock", escape_digits)?);
    Ok(if digit.is_some() {
        digit
    } else if time.minute() == 0 {
        None
    } else {
        say_number(time.minute() as i32, escape_digits)?
    })
}

/// Execute a dialplan application.
pub fn exec(app: &str, options: &str) -> Result<AgiResult, AgiError> {
    let agi = STDIO.lock().unwrap();
    let options = options.trim_matches('"');
    let response = agi.command(&["EXEC", app, &format!("\"{}\"", options)]);
    match response.error {
        Some(error) => Err(error),
        None => Ok(AgiResult {
            code: response.status as u32,
            result: response.result,
            data: response.result_string,
        }),
    }
}

