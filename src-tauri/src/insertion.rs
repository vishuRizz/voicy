// VoiceKey – insertion.rs
// Delivers finalized text to the previously focused application.
//
// TRD §17 strategy:
//  1. Record the target app before VoiceKey's overlay gains any focus.
//  2. Attempt direct accessibility / synthetic-key insertion.
//  3. Fall back to clipboard-paste if direct insertion is unavailable/denied.
//  4. Restore the clipboard if feasible.
//  5. Document limitations honestly.

use crate::{errors::VoiceKeyError, platform};
use anyhow::Result;
use tracing::{info, warn};

/// Words already written into the focused field during this hold.
///
/// Each update deletes only the part that changed and pastes the new tail,
/// so the sentence grows in place instead of waiting for key-up.
pub struct LiveDraft {
    current: String,
    failed: bool,
}

impl LiveDraft {
    pub fn new() -> Self {
        Self {
            current: String::new(),
            failed: false,
        }
    }

    pub fn has_text(&self) -> bool {
        !self.current.is_empty()
    }

    pub fn off(&self) -> bool {
        self.failed
    }

    pub async fn sync(&mut self, next: &str) -> Result<(), VoiceKeyError> {
        if self.failed {
            return Err(VoiceKeyError::InsertionFailed(
                "live insert unavailable".into(),
            ));
        }
        if next == self.current {
            return Ok(());
        }
        let (delete_n, suffix) = changed_suffix(&self.current, next);
        if delete_n == 0 && suffix.is_empty() {
            return Ok(());
        }
        platform::apply_text_edit(delete_n, &suffix).map_err(|e| {
            self.failed = true;
            VoiceKeyError::InsertionFailed(e.to_string())
        })?;
        self.current = next.to_string();
        Ok(())
    }

    pub async fn clear(&mut self) -> Result<(), VoiceKeyError> {
        if self.current.is_empty() {
            return Ok(());
        }
        let n = self.current.chars().count();
        self.current.clear();
        self.failed = true;
        platform::apply_text_edit(n, "")
            .map_err(|e| VoiceKeyError::InsertionFailed(e.to_string()))
    }
}

/// How many characters to delete from the end of `old`, and what to insert
/// after that, so the field becomes `new`.
fn changed_suffix(old: &str, new: &str) -> (usize, String) {
    let old: Vec<char> = old.chars().collect();
    let new: Vec<char> = new.chars().collect();
    let mut i = 0;
    while i < old.len() && i < new.len() && old[i] == new[i] {
        i += 1;
    }
    (old.len() - i, new[i..].iter().collect())
}

/// Insert `text` into the previously focused application.
///
/// Returns `Ok(true)` if direct insertion succeeded, `Ok(false)` if clipboard
/// fallback was used, or an error if both methods failed.
pub async fn insert_text(text: &str, use_clipboard_fallback: bool) -> Result<bool, VoiceKeyError> {
    info!("Attempting direct text insertion ({} chars)", text.len());

    match platform::insert_text_direct(text).await {
        Ok(()) => {
            info!("Direct insertion succeeded");
            Ok(true)
        }
        Err(e) => {
            warn!("Direct insertion failed: {e}");

            if use_clipboard_fallback {
                info!("Trying clipboard fallback");
                platform::insert_text_via_clipboard(text)
                    .await
                    .map_err(|e| VoiceKeyError::InsertionFailed(e.to_string()))?;
                info!("Clipboard fallback succeeded");
                Ok(false)
            } else {
                Err(VoiceKeyError::InsertionFailed(e.to_string()))
            }
        }
    }
}
