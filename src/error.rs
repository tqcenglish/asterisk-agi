//! Error types, status codes and channel states.

use std::io;

use thiserror::Error;

use crate::response::AgiResult;

/// Errors that may occur while talking to Asterisk over AGI.
#[derive(Error, Debug)]
pub enum AgiError {
    /// An underlying I/O operation failed.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// Asterisk reported a negative result for a command.
    #[error("AGI command failed")]
    AgiCommandFailed(AgiResult),

    /// The command produced no parseable response at all.
    #[error("AGI command did not return a result")]
    AgiNoResult,

    /// The channel hung up while a command was in flight.
    #[error("hangup")]
    Hangup,

    /// An argument contained a line break or NUL byte and was refused
    /// before it could inject additional AGI commands.
    #[error("invalid command argument: {0}")]
    InvalidArgument(String),

    /// A response line could not be parsed.
    #[error("failed to parse result: {0}")]
    Parse(String),

    /// Asterisk returned a status code other than 200.
    #[error("non-200 status code: {0}")]
    NonOkStatus(i32),

    /// A FastAGI server could not be bound to the requested address.
    #[error("failed to bind server: {0}")]
    Bind(String),

    /// Accepting a FastAGI connection failed.
    #[error("failed to accept TCP connection: {0}")]
    Accept(String),

    /// The requested MRCP application is not loaded in Asterisk.
    #[error("MRCP applications not loaded")]
    MrcpNotLoaded,

    /// A logger was attached more than once.
    #[error("logger already attached")]
    LoggerAlreadyAttached,

    /// Any other error, carrying a human readable message.
    #[error("{0}")]
    Other(String),
}

/// AGI status codes (HTTP-style) returned by Asterisk.
pub mod status {
    /// The command was accepted.
    pub const OK: i32 = 200;
    /// Asterisk did not understand the command.
    pub const INVALID: i32 = 510;
    /// The command cannot be performed on a dead (hung-up) channel.
    pub const DEAD_CHANNEL: i32 = 511;
    /// Usage has ended.
    pub const END_USAGE: i32 = 520;
}

/// Asterisk channel states, as returned by `CHANNEL STATUS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Channel is down and available.
    Down,
    /// Channel is down but reserved.
    Reserved,
    /// Channel is off hook.
    Offhook,
    /// Digits have been dialed.
    Dialing,
    /// Channel is ringing.
    Ring,
    /// Remote end is ringing (receiving ringback).
    Ringing,
    /// Channel is up.
    Up,
    /// Line is busy.
    Busy,
    /// Digits were dialed while off hook.
    DialingOffHook,
    /// An incoming call was detected, waiting for ring.
    PreRing,
    /// A state not known by this crate.
    Unknown(i32),
}

impl State {
    /// Map a raw numeric channel status to a [`State`].
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => State::Down,
            1 => State::Reserved,
            2 => State::Offhook,
            3 => State::Dialing,
            4 => State::Ring,
            5 => State::Ringing,
            6 => State::Up,
            7 => State::Busy,
            8 => State::DialingOffHook,
            9 => State::PreRing,
            other => State::Unknown(other),
        }
    }
}
