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
