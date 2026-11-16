//! MRCP speech synthesis and recognition helpers.
//!
//! These require the UniMRCP applications and resources to be compiled and
//! loaded in Asterisk.

use crate::error::AgiError;
use crate::session::Agi;

/// Result of an MRCP speech recognition action.
#[derive(Debug, Clone)]
pub struct RecognitionResult {
    /// `RECOG_STATUS`: `OK`, `ERROR` or `INTERRUPTED`.
    pub status: String,
    /// `RECOG_COMPLETION_CAUSE`: 0 success, 1 no match, 2 no input.
    pub cause: i32,
    /// `RECOG_RESULT`: the unparsed NLSML result.
    pub result: String,
}

/// One interpretation of a speech recognition result.
#[derive(Debug, Clone)]
pub struct RecognitionInterpretation {
    /// Confidence on a 0-100 scale, 100 being best.
    pub confidence: i32,
    /// Textual representation of the recognised speech.
    pub input: String,
    /// Grammar or recognition rule that matched.
    pub grammar: String,
}

/// Result of an MRCP synthesis operation.
#[derive(Debug, Clone)]
pub struct SynthResult {
    /// `SYNTHSTATUS`: `OK`, `ERROR` or `INTERRUPTED`.
    pub status: String,
    /// `SYNTH_COMPLETION_CAUSE`: 0 normal, 1 barge-in, 2 parse failure.
    pub cause: i32,
}

impl Agi {
    /// Synthesize speech for a prompt via MRCP.
    pub fn mrcp_synth(&self, prompt: &str, options: &str) -> Result<SynthResult, AgiError> {
        let ret = self.exec(&["MRCPSynth", prompt, options])?;
        if ret == "-2" {
            return Err(AgiError::MrcpNotLoaded);
        }
        Ok(SynthResult {
            status: self.get("SYNTHSTATUS")?,
            cause: self.get("SYNTH_COMPLETION_CAUSE")?.parse().map_err(|_| {
                AgiError::Parse("SYNTH_COMPLETION_CAUSE".to_string())
            })?,
        })
    }

    /// Listen for speech and optionally play a prompt.
    pub fn mrcp_recog(&self, grammar: &str, options: &str) -> Result<RecognitionResult, AgiError> {
        let ret = self.exec(&["MRCPRecog", grammar, options])?;
        if ret == "-2" {
            return Err(AgiError::MrcpNotLoaded);
        }
        self.recognition_result(false)
    }

    /// Play a synthesized prompt and wait for speech to be recognised.
    pub fn synth_and_recog(
        &self,
        prompt: &str,
        grammar: &str,
        options: &str,
    ) -> Result<RecognitionResult, AgiError> {
        let exec_options = format!("\"{}\",{},{}", prompt, grammar, options);
        let ret = self.exec(&["SynthAndRecog", &exec_options])?;
        if ret == "-2" {
            return Err(AgiError::MrcpNotLoaded);
        }
        self.recognition_result(true)
    }

    /// Return the best speech interpretation from the last recognition.
    pub fn recognition_interpretation(
        &self,
        index: usize,
    ) -> Result<RecognitionInterpretation, AgiError> {
        Ok(RecognitionInterpretation {
            input: self.recognition_input(index)?,
            confidence: self.recognition_confidence(index)?,
            grammar: self.recognition_grammar(index)?,
        })
    }

    /// Return the recognised input at `index` (0 is the best match).
    pub fn recognition_input(&self, index: usize) -> Result<String, AgiError> {
        self.get(&format!("RECOG_INPUT({})", index))
    }

    /// Return the confidence (0-100) at `index`.
    pub fn recognition_confidence(&self, index: usize) -> Result<i32, AgiError> {
        self.get(&format!("RECOG_CONFIDENCE({})", index))?
            .parse()
            .map_err(|_| AgiError::Parse("RECOG_CONFIDENCE".to_string()))
    }

    /// Return the matched grammar at `index`.
    pub fn recognition_grammar(&self, index: usize) -> Result<String, AgiError> {
        self.get(&format!("RECOG_GRAMMAR({})", index))
    }

    fn recognition_result(&self, combo: bool) -> Result<RecognitionResult, AgiError> {
        let status_var = if combo { "RECOG_STATUS" } else { "RECOGSTATUS" };
        Ok(RecognitionResult {
            status: self.get(status_var)?,
            cause: self
                .get("RECOG_COMPLETION_CAUSE")?
                .parse()
                .map_err(|_| AgiError::Parse("RECOG_COMPLETION_CAUSE".to_string()))?,
            result: self.get("RECOG_RESULT")?,
        })
    }
}
