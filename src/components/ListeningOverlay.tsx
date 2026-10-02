// VoiceKey – components/ListeningOverlay.tsx
// Compact, always-on-top, non-activating overlay shown during a recording
// session.  Displays the current state, a live provisional transcript, and
// a cancel button.

import React from 'react';
import type { UiState } from '../hooks/useSession';
import type { ErrorEvent } from '../types';
import { humanErrorMessage } from '../types';

interface Props {
  uiState: UiState;
  preview: string;
  finalText: string | null;
  error: ErrorEvent | null;
  onCancel: () => void;
}

const STATE_LABELS: Record<UiState, string> = {
  idle: 'Ready',
  listening: 'Listening…',
  finalizing: 'Processing…',
  inserting: 'Inserting…',
  error: 'Error',
};

export const ListeningOverlay: React.FC<Props> = ({
  uiState,
  preview,
  finalText,
  error,
  onCancel,
}) => {
  const isActive = uiState !== 'idle';

  return (
    <div
      id="listening-overlay"
      className={`overlay ${uiState}`}
      role="status"
      aria-live="polite"
      aria-label={`Voicy: ${STATE_LABELS[uiState]}`}
      style={{ display: isActive ? 'flex' : 'none' }}
    >
      {/* Pulse indicator */}
      <div className="overlay-indicator">
        <span className={`mic-dot ${uiState === 'listening' ? 'pulsing' : ''}`} aria-hidden="true" />
        <span className="overlay-state-label">{STATE_LABELS[uiState]}</span>
      </div>

      {/* Preview text */}
      {uiState === 'listening' && preview && (
        <p className="overlay-preview" aria-label="Live transcript">
          {preview}
        </p>
      )}

      {/* Final text (brief display before insertion) */}
      {uiState === 'inserting' && finalText && (
        <p className="overlay-final">{finalText}</p>
      )}

      {/* Error message */}
      {uiState === 'error' && error && (
        <p className="overlay-error" role="alert">
          {humanErrorMessage(error.code)}
        </p>
      )}

      {/* Cancel button */}
      {(uiState === 'listening' || uiState === 'finalizing') && (
        <button
          id="cancel-recording-btn"
          className="overlay-cancel"
          onClick={onCancel}
          aria-label="Cancel recording (Escape)"
        >
          ✕ Cancel
        </button>
      )}
    </div>
  );
};

export default ListeningOverlay;
