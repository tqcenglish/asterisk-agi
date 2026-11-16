//! This crate helps to implement simple AGI programs.
//! AGI is the Asterisk Gateway Interface
//!
//! When using this crate the first thing you probably have
//! to do is calling `read_variables()` which will read the
//! variables Asterisk provides to the AGI program. After that
//! use the other functions to control what Asterisk is
//! doing with the invocing channel.

use chrono::naive::NaiveTime;
use chrono::{Date, Timelike};
use chrono_tz::Tz;
use lazy_static::lazy_static;
use log::{debug, info, trace};
use regex::Regex;
use std::collections::HashMap;
use std::io;
use std::{thread, time};
use thiserror::Error;

/// Represents a result code from the AGI
#[derive(Debug)]
pub struct AgiResult {
    pub code: u32,
    pub result: i32,
    pub data: String,
}

/// Represents an item presented to the caller in a voice menu.
#[derive(Debug)]
pub enum MenuItem {
    File(String),
    Digits(String),
    Number(i32),
    Date(Date<Tz>),
    Time(NaiveTime),
}

#[derive(Error, Debug)]
pub enum AgiError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("AGI command failed")]
    AgiCommandFailed(AgiResult),

    #[error("AGI command did not return a result")]
    AgiNoResult,
}

/// Pause further execution for the given number of seconds.
pub fn pause(seconds: u64) {
    thread::sleep(time::Duration::from_secs(seconds));
}

/// Read the variables given by Asterisk when it calls an AGI program.
///
/// Typically this is the first call to this crate in an AGI program.
pub fn read_variables() -> io::Result<HashMap<String, String>> {
    lazy_static! {
        static ref VAR_RE: Regex = Regex::new("^([^:]+): (.*)$").unwrap();
    }

    let mut variables: HashMap<String, String> = HashMap::new();
    loop {
        let mut line = String::new();
        trace!("Var line: {}", line);
        let bytes = io::stdin().read_line(&mut line)?;
        if let Some(capture) = VAR_RE.captures(line.trim()) {
            variables.insert(
                capture.get(1).unwrap().as_str().to_string(),
                capture.get(2).unwrap().as_str().to_string(),
            );
        }
        if bytes <= 2 {
            return Ok(variables);
        }
    }
}

/// Get the content of an Asterisk variable
pub fn get_variable(name: &str) -> Result<Option<String>, AgiError> {
    let cmd = format!("GET VARIABLE {}", name);
    let res = send_command(&cmd)?;
    Ok(Some(res.data))
}

/// Set an Asterisk variable
pub fn set_variable(name: &str, value: &str) -> Result<(), AgiError> {
    let cmd = format!("SET VARIABLE {} {}", name, value);
    send_command(&cmd)?;
    Ok(())
}

/// Parse the result of a command sent to Asterisk
fn parse_result(res: &str) -> Result<AgiResult, AgiError> {
    lazy_static! {
        static ref RES_RE: Regex = Regex::new("^(\\d{3}) result=([^ ]+)( .*)?$").unwrap();
    }

    let raw_result = RES_RE.captures(res).map(|c| AgiResult {
        code: c
            .get(1)
            .map(|code| code.as_str().parse().unwrap_or(555))
            .unwrap_or(555),
        result: c
            .get(2)
            .map(|result| result.as_str().parse().unwrap_or(-1))
            .unwrap_or(-1),
        data: c
            .get(3)
            .map(|data| data.as_str().trim().to_string())
            .unwrap_or(String::new()),
    });

    match raw_result {
        Some(result) => {
            if result.result < 0 {
                return Err(AgiError::AgiCommandFailed(result));
            }
            Ok(result)
        },
        None => Err(AgiError::AgiNoResult),
    }
}

/// Send a command to Asterisk
fn send_command(cmd: &str) -> Result<AgiResult, AgiError> {
    debug!("AGI cmd: {}", cmd);
    println!("{}", cmd);

    let mut result = String::new();
    io::stdin().read_line(&mut result)?;
    let result = parse_result(result.trim());
    debug!("Result: {:?}", result);
    result
}

/// Answer a call
pub fn answer() -> Result<(), AgiError> {
    send_command("ANSWER")?;
    pause(2);
    Ok(())
}

/// Play an audio file to the caller
pub fn stream_file(file: &str, escape_digits: &str) -> Result<Option<char>, AgiError> {
    let cmd = format!("STREAM FILE {} \"{}\"", file, escape_digits);
    let result = send_command(&cmd)?;
    Ok(if result.result == 0 {
        None
    } else {
        Some(result.result as u8 as char)
    })
}

/// Wait for the caller to press a key
pub fn wait_for_digit(timeout_ms: i32) -> Result<Option<char>, AgiError> {
    let wait_cmd = format!("WAIT FOR DIGIT {}", timeout_ms);
    let res = send_command(&wait_cmd)?;
    Ok(Some(res.result as u32)
        .filter(|&c| c == 0x2A || c == 0x23 || 0x30 <= c && c <= 0x39)
        .map(|d| d as u8 as char))
}

/// Play an audio file and wait for digits to be pressed by the caller
pub fn prompt(msg_file: &str, answer_digits: &str, timeout_ms: i32) -> Result<Option<char>, AgiError> {
    let res = stream_file(msg_file, answer_digits)?;
    if res.is_some() {
        return Ok(res);
    }
    Ok(wait_for_digit(timeout_ms)?.filter(|&key| answer_digits.contains(key)))
}

/// Play an audio file and wait for a phone number to be entered by the caller
///
/// Entering the phone number either ends by the given `digit_timeout_ms` or
/// by the caller pressing the # key.
pub fn prompt_phonenumber(
    msg_file: &str,
    timeout_ms: i32,
    digit_timeout_ms: i32,
) -> Result<Option<String>, AgiError> {
    let mut phonenumber = String::new();

    let first_digit = prompt(msg_file, "1234567890", timeout_ms)?;
    if let Some(digit) = first_digit {
        phonenumber.push(digit);
    } else {
        return Ok(None);
    }

    'digits: loop {
        let next_digit = wait_for_digit(digit_timeout_ms)?;
        match next_digit {
            None => break 'digits,
            Some('#') => break 'digits,
            Some(digit) => phonenumber.push(digit),
        }
    }

    Ok(Some(phonenumber))
}

/// Present a voice menu to the caller.
///
/// A voice menu may consist of multiple things to be played to
/// the caller. In contrast to playing all the items with individual
/// comands, the caller is able to press any of the keys in `answer_digits`
/// during the menu is played.
pub fn menu(
    messages: &[MenuItem],
    answer_digits: &str,
    timeout_ms: i32,
) -> Result<Option<char>, AgiError> {
    for message in messages {
        if let Some(res) = match message {
            MenuItem::File(f) => stream_file(f, answer_digits)?,
            MenuItem::Digits(d) => say_digits(&d, answer_digits)?,
            MenuItem::Number(n) => say_number(*n, answer_digits)?,
            MenuItem::Date(d) => say_date(d, answer_digits)?,
            MenuItem::Time(t) => say_time(t, answer_digits)?,
        } {
            info!("Menu Result: {}", res);
            return Ok(Some(res));
        }
    }
    Ok(if timeout_ms != 0 {
        wait_for_digit(timeout_ms)?.filter(|&key| answer_digits.contains(key))
    } else {
        None
    })
}

/// Get the optional second digit of the hour
fn get_second_hour_digit(first_digit: u32) -> Result<Option<u32>, AgiError> {
    let second_digit = wait_for_digit(5000)?
        .filter(|&c| '0' <= c && c <= '9')
        .map(|d| (d as u8 - 0x30) as u32);
    Ok(second_digit
        .map(|d| first_digit * 10 + d)
        .filter(|&h| h < 24))
}

/// Plays a sound file to the caller and then waits for an hour to be
/// chosen
pub fn prompt_hour(msg_file: &str) -> Result<Option<u32>, AgiError> {
    let res = prompt(msg_file, "1234567890", 5000)?;
    if let Some(digit) = res {
        return match digit {
            '0' => get_second_hour_digit(0),
            '1' => get_second_hour_digit(1),
            '2' => get_second_hour_digit(2),
            '3' => Ok(Some(3)),
            '4' => Ok(Some(4)),
            '5' => Ok(Some(5)),
            '6' => Ok(Some(6)),
            '7' => Ok(Some(7)),
            '8' => Ok(Some(8)),
            '9' => Ok(Some(9)),
            _ => Ok(None),
        };
    } else {
        return Ok(None);
    }
}

/// Plays the individual digits of a number
pub fn say_digits(digits: &str, escape_digits: &str) -> Result<Option<char>, AgiError> {
    let cmd = format!("SAY DIGITS {} \"{}\"", digits, escape_digits);
    let result = send_command(&cmd)?;
    Ok(if result.result == 0 {
        None
    } else {
        Some(result.result as u8 as char)
    })
}

/// Plays a number
pub fn say_number(number: i32, escape_digits: &str) -> Result<Option<char>, AgiError> {
    let cmd = format!("SAY NUMBER {} \"{}\"", number, escape_digits);
    let result = send_command(&cmd)?;
    Ok(if result.result == 0 {
        None
    } else {
        Some(result.result as u8 as char)
    })
}

/// Plays the date
pub fn say_date(date: &Date<Tz>, escape_digits: &str) -> Result<Option<char>, AgiError> {
    let seconds_since_1970 = date.and_hms(6, 0, 0).timestamp();
    info!("{:?} is {} seconds since 1970", date, seconds_since_1970);
    let cmd = format!(
        "SAY DATETIME {} \"{}\" dBY Europe/Berlin",
        seconds_since_1970, escape_digits
    );
    let result = send_command(&cmd)?;
    Ok(if result.result == 0 {
        None
    } else {
        Some(result.result as u8 as char)
    })
}

/// Plays the time
pub fn say_time(time: &NaiveTime, escape_digits: &str) -> Result<Option<char>, AgiError> {
    let hour = time.hour();
    let minute = time.minute();

    Ok(say_number(hour as i32, escape_digits)?
        .or(stream_file("digits/oclock", escape_digits)?)
        .or(if minute == 0 {
            None
        } else {
            say_number(minute as i32, escape_digits)?
        }))
}

/// Executes a dialplan application
pub fn exec(app: &str, options: &str) -> Result<AgiResult, AgiError> {
    let cmd = format!("EXEC {} \"{}\"", app, options);
    send_command(&cmd)
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
