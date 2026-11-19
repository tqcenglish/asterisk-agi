use std::collections::HashMap;

use asterisk_agi::{Agi, AgiError};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub before_worknumber_audio_path: String,
    pub after_worknumber_audio_path: String,
    pub worknumber_extension_map: HashMap<String, String>,
}

pub fn handle(agi: &Agi, config: &Config) -> Result<(), AgiError> {
    let membername = agi.get("MEMBERNAME")?;
    let mut matches = config
        .worknumber_extension_map
        .iter()
        .filter(|(_, extension)| extension.as_str() == membername);
    let (worknumber, _) = matches.next().ok_or_else(|| {
        AgiError::Other(format!("no worknumber configured for extension '{membername}'"))
    })?;

    if matches.next().is_some() {
        return Err(AgiError::Other(format!(
            "extension '{membername}' matches multiple worknumbers"
        )));
    }
    if worknumber.is_empty() || !worknumber.chars().all(|digit| digit.is_ascii_digit()) {
        return Err(AgiError::Other(format!(
            "configured worknumber for extension '{membername}' must contain digits"
        )));
    }

    agi.answer()?;
    if !config.before_worknumber_audio_path.is_empty() {
        agi.stream_file(&config.before_worknumber_audio_path, "", 0)?;
    }

    agi.say_digits(worknumber, "")?;

    if !config.after_worknumber_audio_path.is_empty() {
        agi.stream_file(&config.after_worknumber_audio_path, "", 0)?;
    }

    Ok(())
}
