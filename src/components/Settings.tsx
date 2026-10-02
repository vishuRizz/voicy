// VoiceKey – components/Settings.tsx
// Full settings panel: shortcut picker, model selector, language, toggles.

import React, { useState, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import type { Settings as SettingsType, WhisperModel, ModelStatus } from '../types';
import myIcon from '../assets/myicon.png';
import { ShortcutRecorder } from './ShortcutRecorder';

const QUALITY_OPTIONS: { value: WhisperModel; label: string; desc: string }[] = [
  { value: 'tiny',   label: 'Fast',      desc: 'Quick, best for short phrases' },
  { value: 'base',   label: 'Balanced',  desc: 'Good accuracy, low latency' },
  { value: 'small',  label: 'Accurate',  desc: 'High accuracy, recommended' },
  { value: 'medium', label: 'Best',      desc: 'Maximum accuracy, slower' },
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
  const [downloading, setDownloading] = useState(false);
  const [downloadPct, setDownloadPct] = useState(0);
  const [downloadError, setDownloadError] = useState<string | null>(null);

  // Sync if parent settings change (e.g. loaded from disk)
  useEffect(() => {
    setDraft(settings);
    setDirty(false);
  }, [settings]);

  // Re-check model status whenever the selected quality changes
  useEffect(() => {
    setDownloadError(null);
    // Temporarily save draft model to state so get_model_status reads the right file
    invoke<ModelStatus>('get_model_status').then(setModelStatus).catch(console.error);
  }, [draft.model]);

  function update<K extends keyof SettingsType>(key: K, val: SettingsType[K]) {
    setDraft((d) => ({ ...d, [key]: val }));
    setDirty(true);
  }

  const handleSave = async () => {
    await onSave(draft);
    setDirty(false);
  };

  // Save settings first (so download_model reads the right model), then download.
  const handleSaveAndDownload = async () => {
    setDownloadError(null);
    // 1. Save settings so the Rust side knows which model to download
    await onSave(draft);
    setDirty(false);

    // 2. Start download with live progress
    setDownloading(true);
    setDownloadPct(0);

    const unlisten = await listen<{ pct: number; done: boolean; error?: string }>(
      'voicekey://download-progress',
      (e) => {
        setDownloadPct(e.payload.pct);
        if (e.payload.done) {
          setDownloading(false);
          invoke<ModelStatus>('get_model_status').then(setModelStatus).catch(console.error);
          unlisten();
        }
        if (e.payload.error) {
          setDownloadError(e.payload.error);
          setDownloading(false);
          unlisten();
        }
      }
    );

    try {
      await invoke('download_model');
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      setDownloadError(msg);
      setDownloading(false);
      unlisten();
    }
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
          <label className="settings-label">Global shortcut</label>
          <ShortcutRecorder
            id="shortcut-input"
            value={draft.shortcut}
            onChange={(s) => update('shortcut', s)}
          />
        </div>
        <div className="shortcut-footer">
          <p className="settings-hint">
            Click the badge and press your key combo. Hold to record, release to insert.
          </p>
          {draft.shortcut !== 'Alt+Space' && (
            <button
              className="btn-ghost btn-xs"
              onClick={() => update('shortcut', 'Alt+Space')}
            >
              Reset to ⌥Space
            </button>
          )}
        </div>
      </section>

      {/* ── Speech Quality ─────────────────────────────────────────────── */}
      <section className="settings-section">
        <h3 className="settings-section-title">Speech Quality</h3>
        <div className="quality-grid">
          {QUALITY_OPTIONS.map((opt) => {
            const isCurrent = draft.model === opt.value;
            return (
              <button
                key={opt.value}
                id={`quality-${opt.value}-btn`}
                className={`quality-card ${isCurrent ? 'selected' : ''}`}
                onClick={() => update('model', opt.value)}
                aria-pressed={isCurrent}
                aria-label={`${opt.label} — ${opt.desc}`}
              >
                <span className="quality-label">{opt.label}</span>
                <span className="quality-desc">{opt.desc}</span>
              </button>
            );
          })}
        </div>

        {/* Download required card — shown when selected model isn't installed */}
        {modelStatus && !modelStatus.installed && !downloading && (
          <div className="download-required-card" role="status">
            <div className="download-required-icon">⬇</div>
            <div className="download-required-body">
              <p className="download-required-title">Download required</p>
              <p className="download-required-sub">
                This quality level needs to be downloaded before use.
                Your settings will be saved automatically.
              </p>
              {downloadError && (
                <p className="download-required-error">
                  ✕ {downloadError}
                </p>
              )}
            </div>
            <button
              id="download-model-btn"
              className="btn-primary btn-download"
              onClick={handleSaveAndDownload}
              disabled={saving}
            >
              {saving ? 'Saving…' : downloadError ? 'Retry' : 'Save & Download'}
            </button>
          </div>
        )}

        {/* Download progress */}
        {downloading && (
          <div className="download-progress-card" role="status" aria-live="polite">
            <div className="download-progress-header">
              <span className="download-progress-title">Downloading…</span>
              <span className="download-progress-pct">{Math.round(downloadPct)}%</span>
            </div>
            <div className="progress-bar">
              <div className="progress-fill" style={{ width: `${downloadPct}%` }} />
            </div>
            <p className="download-progress-sub">
              Do not close the app. This may take a few minutes.
            </p>
          </div>
        )}

        {/* Ready badge */}
        {modelStatus?.installed && !downloading && (
          <div className="model-ready-badge">
            <span className="model-ready-dot" />
            <span>Ready to use</span>
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
