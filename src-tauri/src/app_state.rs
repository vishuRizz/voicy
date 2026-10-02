// VoiceKey – app_state.rs
// Owns the recording state machine and session identifiers.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// The recording / session state machine.
///
/// ```
/// IDLE → (hotkey down) → LISTENING → (hotkey up) → FINALIZING → (done) → INSERTING → IDLE
///                           │                                                  │
///                       (cancel/error)                                    (success/fallback)
///                           ↓                                                  ↓
///                          IDLE                                              IDLE
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionState {
    Idle,
    Listening,
    Finalizing,
    Inserting,
    Error(String),
}

impl Default for SessionState {
    fn default() -> Self {
        SessionState::Idle
    }
}

/// A unique session.  Every hold-to-talk press generates a new session ID so
/// that stale inference results from a previous session can be safely ignored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub state: SessionState,
    /// Provisional transcript shown while listening.
    pub preview: String,
    /// Authoritative transcript set when finalizing completes.
    pub final_text: Option<String>,
}

impl Session {
    pub fn new() -> Self {
        Session {
            id: Uuid::new_v4().to_string(),
            state: SessionState::Listening,
            preview: String::new(),
            final_text: None,
        }
    }
}

/// Shared application state – Arc<RwLock<AppState>> is passed into Tauri's
/// manage() call so every Tauri command handler can access it.
#[derive(Debug, Default)]
pub struct AppState {
    /// The current active session, or None when idle.
    pub session: Option<Session>,
}

pub type SharedAppState = Arc<RwLock<AppState>>;

impl AppState {
    pub fn new_shared() -> SharedAppState {
        Arc::new(RwLock::new(AppState::default()))
    }

    // ── state machine helpers ───────────────────────────────────────────────

    /// Transition to LISTENING; creates a fresh session.
    pub fn start_listening(&mut self) -> String {
        let s = Session::new();
        let id = s.id.clone();
        self.session = Some(s);
        id
    }

    /// Update the provisional preview for the current session (if still
    /// LISTENING) and only if the session_id matches.
    pub fn update_preview(&mut self, session_id: &str, text: String) {
        if let Some(ref mut s) = self.session {
            if s.id == session_id && s.state == SessionState::Listening {
                s.preview = text;
            }
        }
    }

    /// Transition to FINALIZING.
    pub fn begin_finalizing(&mut self, session_id: &str) {
        if let Some(ref mut s) = self.session {
            if s.id == session_id && s.state == SessionState::Listening {
                s.state = SessionState::Finalizing;
            }
        }
    }

    /// Transition to INSERTING with the final text.
    pub fn set_final_text(&mut self, session_id: &str, text: String) {
        if let Some(ref mut s) = self.session {
            if s.id == session_id && s.state == SessionState::Finalizing {
                s.final_text = Some(text);
                s.state = SessionState::Inserting;
            }
        }
    }

    /// Return to IDLE and drop the session.
    pub fn finish(&mut self) {
        self.session = None;
    }

    /// Cancel the current session and return to IDLE.
    pub fn cancel(&mut self) {
        self.session = None;
    }

    /// Set an error on the current session (if session id matches).
    pub fn set_error(&mut self, session_id: &str, msg: String) {
        if let Some(ref mut s) = self.session {
            if s.id == session_id {
                s.state = SessionState::Error(msg);
            }
        }
    }

    // ── read helpers ────────────────────────────────────────────────────────

    pub fn current_session_id(&self) -> Option<String> {
        self.session.as_ref().map(|s| s.id.clone())
    }

    pub fn current_state(&self) -> SessionState {
        self.session
            .as_ref()
            .map(|s| s.state.clone())
            .unwrap_or(SessionState::Idle)
    }

    pub fn is_listening(&self) -> bool {
        matches!(self.current_state(), SessionState::Listening)
    }
}
