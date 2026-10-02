// VoiceKey – components/Settings.tsx
// Full settings panel: shortcut picker, model selector, language, toggles.

import React, { useState, useEffect } from 'react';
import type { Settings as SettingsType, WhisperModel, ModelStatus } from '../types';
import { getModelStatus } from '../lib/tauri';
import myIcon from '../assets/myicon.png';

const MODEL_OPTIONS: { value: WhisperModel; label: string; sizeMb: number }[] = [
  { value: 'tiny', label: 'Tiny (~75 MB)', sizeMb: 75 },
  { value: 'base', label: 'Base (~142 MB)', sizeMb: 142 },
  { value: 'small', label: 'Small (~466 MB)', sizeMb: 466 },
  { value: 'medium', label: 'Medium (~1.5 GB)', sizeMb: 1457 },
];

const LANGUAGE_OPTIONS = [
  { value: 'en', label: 'English' },
  { value: 'fr', label: 'French' },
  { value: 'de', label: 'German' },
  { value: 'es', label: 'Spanish' },
  { value: 'it', label: 'Italian' },
  { value: 'pt', label: 'Portuguese' },
  { value: 'ja', label: 'Japanese' },
  { value: 'zh', label: 'Chinese' },
  { value: 'auto', label: 'Auto-detect' },
];

interface Props {
  settings: SettingsType;
  saving: boolean;
  onSave: (s: SettingsType) => Promise<void>;
}

export const SettingsPanel: React.FC<Props> = ({ settings, saving, onSave }) => {
  const [draft, setDraft] = useState<SettingsType>(settings);
  const [modelStatus, setModelStatus] = useState<ModelStatus | null>(null);
  const [dirty, setDirty] = useState(false);

  // Sync if parent settings change (e.g. loaded from disk)
  useEffect(() => {
    setDraft(settings);
    setDirty(false);
  }, [settings]);

  useEffect(() => {
    getModelStatus().then(setModelStatus).catch(console.error);
  }, [draft.model]);

  function update<K extends keyof SettingsType>(key: K, val: SettingsType[K]) {
    setDraft((d) => ({ ...d, [key]: val }));
    setDirty(true);
  }

  const handleSave = async () => {
    await onSave(draft);
    setDirty(false);
  };

  return (
    <div id="settings-panel" className="settings-panel">
      <div className="settings-header">
        <img src={myIcon} alt="VoiceKey" className="settings-logo" />
        <div>
          <h2 className="settings-title">VoiceKey</h2>
          <p className="settings-subtitle">Settings</p>
        </div>
      </div>

      {/* ── Shortcut ──────────────────────────────────────────────────── */}
      <section className="settings-section">
        <h3 className="settings-section-title">Hold-to-Talk Shortcut</h3>
        <div className="settings-row">
          <label htmlFor="shortcut-input" className="settings-label">
            Global shortcut
          </label>
          <input
            id="shortcut-input"
            className="settings-input shortcut-input"
            type="text"
            value={draft.shortcut}
            onChange={(e) => update('shortcut', e.target.value)}
            placeholder="e.g. Option+Space"
            aria-label="Global hold-to-talk shortcut"
          />
        </div>
        <p className="settings-hint">
          Hold this key to record. Release to transcribe and insert.
        </p>
      </section>

      {/* ── Model ─────────────────────────────────────────────────────── */}
      <section className="settings-section">
        <h3 className="settings-section-title">Whisper Model</h3>
        <div className="model-grid">
          {MODEL_OPTIONS.map((opt) => (
            <button
              key={opt.value}
              id={`model-${opt.value}-btn`}
              className={`model-card ${draft.model === opt.value ? 'selected' : ''}`}
              onClick={() => update('model', opt.value)}
              aria-pressed={draft.model === opt.value}
              aria-label={`Select ${opt.label} model`}
            >
              <span className="model-name">{opt.value.charAt(0).toUpperCase() + opt.value.slice(1)}</span>
              <span className="model-size">{opt.label.match(/\(.*\)/)?.[0] ?? ''}</span>
            </button>
          ))}
        </div>

        {modelStatus && (
          <div className={`model-status ${modelStatus.installed ? 'installed' : 'missing'}`}>
            {modelStatus.installed ? (
              <span>✓ Model installed</span>
            ) : (
              <span>
                ⚠ Model not installed (~{modelStatus.size_mb} MB required).{' '}
                <a
                  href="https://huggingface.co/ggerganov/whisper.cpp"
                  target="_blank"
                  rel="noreferrer"
                  className="link"
                  id="download-model-link"
                >
                  Download
                </a>
              </span>
            )}
          </div>
        )}
      </section>

      {/* ── Language ──────────────────────────────────────────────────── */}
      <section className="settings-section">
        <h3 className="settings-section-title">Language</h3>
        <div className="settings-row">
          <label htmlFor="language-select" className="settings-label">Recognition language</label>
          <select
            id="language-select"
            className="settings-select"
            value={draft.language}
            onChange={(e) => update('language', e.target.value)}
          >
            {LANGUAGE_OPTIONS.map((l) => (
              <option key={l.value} value={l.value}>{l.label}</option>
            ))}
          </select>
        </div>
      </section>

      {/* ── Toggles ───────────────────────────────────────────────────── */}
      <section className="settings-section">
        <h3 className="settings-section-title">Behaviour</h3>

        <div className="settings-row toggle-row">
          <div>
            <label htmlFor="clipboard-fallback-toggle" className="settings-label">
              Clipboard fallback
            </label>
            <p className="settings-hint">
              When direct insertion fails, paste via clipboard.
            </p>
          </div>
          <label className="toggle" aria-label="Clipboard fallback">
            <input
              id="clipboard-fallback-toggle"
              type="checkbox"
              checked={draft.clipboard_fallback}
              onChange={(e) => update('clipboard_fallback', e.target.checked)}
            />
            <span className="toggle-track" />
          </label>
        </div>

        <div className="settings-row toggle-row">
          <div>
            <label htmlFor="show-overlay-toggle" className="settings-label">
              Show overlay while recording
            </label>
            <p className="settings-hint">
              Display a compact indicator while recording.
            </p>
          </div>
          <label className="toggle" aria-label="Show overlay">
            <input
              id="show-overlay-toggle"
              type="checkbox"
              checked={draft.show_overlay}
              onChange={(e) => update('show_overlay', e.target.checked)}
            />
            <span className="toggle-track" />
          </label>
        </div>

        <div className="settings-row">
          <div>
            <label htmlFor="max-recording-input" className="settings-label">
              Max recording duration (seconds)
            </label>
            <p className="settings-hint">Set to 0 for unlimited.</p>
          </div>
          <input
            id="max-recording-input"
            type="number"
            min={0}
            max={300}
            step={5}
            className="settings-input number-input"
            value={draft.max_recording_secs}
            onChange={(e) => update('max_recording_secs', Number(e.target.value))}
          />
        </div>
      </section>

      {/* ── Save ──────────────────────────────────────────────────────── */}
      <div className="settings-footer">
        <button
          id="save-settings-btn"
          className={`btn-primary ${!dirty ? 'btn-disabled' : ''}`}
          disabled={!dirty || saving}
          onClick={handleSave}
          aria-label="Save settings"
        >
          {saving ? 'Saving…' : dirty ? 'Save Changes' : 'Saved'}
        </button>
      </div>
    </div>
  );
};

export default SettingsPanel;
