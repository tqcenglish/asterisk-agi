//! AGI response parsing.

use lazy_static::lazy_static;
use regex::Regex;

use crate::error::{AgiError, status};

/// Legacy result container kept for backwards compatibility with the
/// original function-based API.
#[derive(Debug, Clone)]
pub struct AgiResult {
    /// HTTP-style status code (200 on success).
    pub code: u32,
    /// Numeric result, or -1 when it could not be parsed.
    pub result: i32,
    /// Trailing data portion of the response.
    pub data: String,
}

/// A parsed response to an AGI command.
///
/// Mirrors the Go implementation: the numeric `result`, its raw string form
/// `result_string`, and the optional parenthesised `value` are all retained,
/// alongside a possible `error`.
#[derive(Debug)]
pub struct Response {
    /// Error encountered while issuing or parsing the command, if any.
    pub error: Option<AgiError>,
    /// HTTP-style status code reported by Asterisk.
    pub status: i32,
    /// Numeric result, -1 when the value was not an integer.
    pub result: i32,
    /// The raw `result=` value as returned by Asterisk.
    pub result_string: String,
    /// The optional trailing field, with surrounding parentheses removed.
    pub value: String,
}

impl Default for Response {
    fn default() -> Self {
        Response {
            error: None,
            status: 0,
            result: 0,
            result_string: String::new(),
            value: String::new(),
        }
    }
}

impl Response {
    /// Return the raw result string, or the error that prevented it.
    pub fn res(&self) -> Result<&str, &AgiError> {
        match &self.error {
            Some(e) => Err(e),
            None => Ok(&self.result_string),
        }
    }

    /// Return the error associated with this response, if any.
    pub fn err(&self) -> Option<&AgiError> {
        self.error.as_ref()
    }

    /// Return the optional value field, or the error that prevented it.
    pub fn val(&self) -> Result<&str, &AgiError> {
        match &self.error {
            Some(e) => Err(e),
            None => Ok(&self.value),
        }
    }

    /// Whether the command completed without error.
    pub fn is_ok(&self) -> bool {
        self.error.is_none()
    }
}

lazy_static! {
    // `^([\d]{3})\sresult=(\-?[[:alnum:]]*)(\s.*)?$` from the Go version.
    static ref RESPONSE_RE: Regex =
        Regex::new(r"^(\d{3})\sresult=(-?[[:alnum:]]*)(?:\s(.*))?$").unwrap();
}

/// Parse a single AGI response line into a [`Response`].
pub fn parse_response(raw: &str) -> Result<Response, AgiError> {
    let line = raw.trim();
    let captures = RESPONSE_RE
        .captures(line)
        .ok_or_else(|| AgiError::Parse(line.to_string()))?;

    let status_code: i32 = match captures[1].parse() {
        Ok(code) => code,
        Err(_) => return Err(AgiError::Parse(line.to_string())),
    };

    let result_string = captures[2].to_string();
    let result = result_string.parse().unwrap_or(-1);

    let mut value = captures.get(3).map_or("", |m| m.as_str()).trim().to_string();
    if let Some(stripped) = value.strip_prefix('(') {
        value = stripped.to_string();
    }
    if let Some(stripped) = value.strip_suffix(')') {
        value = stripped.to_string();
    }

    let mut response = Response {
        error: None,
        status: status_code,
        result,
        result_string,
        value,
    };

    if status_code != status::OK {
        response.error = Some(AgiError::NonOkStatus(status_code));
    }

    Ok(response)
}

/// Parse a response into the legacy [`AgiResult`] shape used by the
/// function-based API.
pub fn parse_legacy_result(raw: &str) -> Result<AgiResult, AgiError> {
    let response = parse_response(raw)?;
    let legacy = AgiResult {
        code: response.status as u32,
        result: response.result,
        data: response.value.clone(),
    };
    if response.result < 0 {
        return Err(AgiError::AgiCommandFailed(legacy));
    }
    Ok(legacy)
}

/// Map an AGI `result=` ASCII code to the corresponding DTMF digit string.
///
/// `result=0` (or any non-printable value) maps to an empty string. This is
/// the equivalent of the Go `dtmfDigit` helper: the digit lives in `result=`,
/// not in the trailing value field (which often holds `endpos=...`).
pub fn dtmf_digit(response: Response) -> Result<String, AgiError> {
    if let Some(error) = response.error {
        return Err(error);
    }
    if let Some(ch) = char::from_u32(response.result as u32) {
        if !ch.is_control() && ch != '\u{7f}' {
            return Ok(ch.to_string());
        }
    }
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_and_value() {
        let r = parse_response("200 result=1 (endpos=21760)").unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.result, 1);
        assert_eq!(r.result_string, "1");
        assert_eq!(r.value, "endpos=21760");
        assert!(r.is_ok());
    }

    #[test]
    fn parses_negative_result() {
        let r = parse_response("200 result=-1").unwrap();
        assert_eq!(r.result, -1);
        assert_eq!(r.result_string, "-1");
        assert_eq!(r.value, "");
    }

    #[test]
    fn non_200_is_an_error() {
        let r = parse_response("511 result=0").unwrap();
        assert!(matches!(r.error, Some(AgiError::NonOkStatus(511))));
    }

    #[test]
    fn garbage_is_a_parse_error() {
        assert!(matches!(parse_response("garbage"), Err(AgiError::Parse(_))));
    }

    #[test]
    fn dtmf_digit_maps_ascii_codes() {
        assert_eq!(dtmf_digit(parse_response("200 result=54 endpos=21760").unwrap()).unwrap(), "6");
        assert_eq!(dtmf_digit(parse_response("200 result=56").unwrap()).unwrap(), "8");
        assert_eq!(dtmf_digit(parse_response("200 result=35").unwrap()).unwrap(), "#");
        assert_eq!(dtmf_digit(parse_response("200 result=0 endpos=80017").unwrap()).unwrap(), "");
    }

    #[test]
    fn dtmf_digit_propagates_errors() {
        let mut r = Response::default();
        r.error = Some(AgiError::Hangup);
        assert!(matches!(dtmf_digit(r), Err(AgiError::Hangup)));
    }
}
