// VoiceKey – shared TypeScript types matching the Rust event/command contracts.

// ── Session ─────────────────────────────────────────────────────────────────

export type SessionState =
  | 'IDLE'
  | 'LISTENING'
  | 'FINALIZING'
  | 'INSERTING'
  | `ERROR:${string}`;

export interface StateEvent {
  session_id: string;
  state: string;
}

export interface PreviewEvent {
  session_id: string;
  text: string;
  is_provisional: boolean;
}

export interface FinalEvent {
  session_id: string;
  text: string;
}

export interface ErrorEvent {
  session_id: string;
  code: string;
  message: string;
}

// ── Settings ─────────────────────────────────────────────────────────────────

export type WhisperModel = 'tiny' | 'base' | 'small' | 'medium';

export interface Settings {
  shortcut: string;
  model: WhisperModel;
  language: string;
  clipboard_fallback: boolean;
  show_overlay: boolean;
  max_recording_secs: number;
}

export const DEFAULT_SETTINGS: Settings = {
  shortcut: 'Option+Space',
  model: 'base',
  language: 'en',
  clipboard_fallback: true,
  show_overlay: true,
  max_recording_secs: 0,
};

// ── Permissions ───────────────────────────────────────────────────────────────

export type PermissionState = 'granted' | 'denied' | 'notdetermined';

export interface PermissionStatus {
  microphone: PermissionState;
  accessibility: PermissionState;
}

// ── Model status ──────────────────────────────────────────────────────────────

export interface ModelStatus {
  model: string;
  installed: boolean;
  path: string | null;
  size_mb: number;
}

// ── Error codes ───────────────────────────────────────────────────────────────

export const ERROR_CODES = {
  MIC_PERMISSION_DENIED: 'MIC_PERMISSION_DENIED',
  MIC_DEVICE_UNAVAILABLE: 'MIC_DEVICE_UNAVAILABLE',
  HOTKEY_REGISTRATION_FAILED: 'HOTKEY_REGISTRATION_FAILED',
  ASR_MODEL_MISSING: 'ASR_MODEL_MISSING',
  ASR_INFERENCE_FAILED: 'ASR_INFERENCE_FAILED',
  INSERTION_PERMISSION_MISSING: 'INSERTION_PERMISSION_MISSING',
  INSERTION_UNSUPPORTED: 'INSERTION_UNSUPPORTED',
  INSERTION_FAILED: 'INSERTION_FAILED',
  SESSION_CANCELLED: 'SESSION_CANCELLED',
} as const;

export type ErrorCode = keyof typeof ERROR_CODES;

export function humanErrorMessage(code: string): string {
  const messages: Record<string, string> = {
    MIC_PERMISSION_DENIED:
      'Microphone access denied. Open System Settings → Privacy → Microphone.',
    MIC_DEVICE_UNAVAILABLE: 'No microphone found. Please connect one and try again.',
    HOTKEY_REGISTRATION_FAILED:
      'Hotkey could not be registered. Another app may be using it.',
    ASR_MODEL_MISSING:
      'Whisper model not found. Please install a model in Settings.',
    ASR_INFERENCE_FAILED: 'Transcription failed. Please try again.',
    INSERTION_PERMISSION_MISSING:
      'Accessibility access denied. Open System Settings → Privacy → Accessibility.',
    INSERTION_UNSUPPORTED:
      'Text could not be inserted into that app. Clipboard fallback was used.',
    INSERTION_FAILED: 'Could not insert text. Please paste manually.',
    SESSION_CANCELLED: 'Recording cancelled.',
  };
  return messages[code] ?? 'An unexpected error occurred.';
}
