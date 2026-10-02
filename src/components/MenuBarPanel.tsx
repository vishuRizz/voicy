// Menu bar panel shown when the macOS status icon is clicked.

import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useSession } from '../hooks/useSession';
import { useSettings } from '../hooks/useSettings';
import { MadeBy } from './MadeBy';
import { getModelStatus, getPermissionStatus } from '../lib/tauri';
import type { PermissionStatus, WhisperModel } from '../types';
import myIcon from '../assets/myicon.png';

const QUALITY: { value: WhisperModel; label: string; hint: string }[] = [
  { value: 'tiny', label: 'Fast', hint: 'Short phrases' },
  { value: 'base', label: 'Balanced', hint: 'Everyday' },
  { value: 'small', label: 'Accurate', hint: 'Recommended' },
  { value: 'medium', label: 'Best', hint: 'Slower' },
];

const LANGUAGES = [
  { value: 'en', label: 'EN' },
  { value: 'hinglish', label: 'Hing' },
  { value: 'fr', label: 'FR' },
  { value: 'de', label: 'DE' },
  { value: 'es', label: 'ES' },
  { value: 'it', label: 'IT' },
  { value: 'pt', label: 'PT' },
  { value: 'ja', label: 'JA' },
  { value: 'zh', label: 'ZH' },
  { value: 'auto', label: 'Auto' },
];

function shortcutKeys(shortcut: string): string[] {
  const symbols: Record<string, string> = {
    Super: '⌘',
    Alt: '⌥',
    Option: '⌥',
    Ctrl: '⌃',
    Control: '⌃',
    Shift: '⇧',
    Space: 'Space',
  };
  return shortcut.split('+').map((part) => symbols[part] ?? part);
}

export const MenuBarPanel: React.FC = () => {
  const { uiState, preview } = useSession();
  const { settings, saving, save } = useSettings();
  const [installed, setInstalled] = useState<Record<string, boolean>>({});
  const [permissions, setPermissions] = useState<PermissionStatus | null>(null);

  useEffect(() => {
    document.documentElement.classList.add('menu-mode');
    return () => document.documentElement.classList.remove('menu-mode');
  }, []);

  useEffect(() => {
    let timer: number | undefined;
    const unlisten = getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      window.clearTimeout(timer);
      if (!focused) {
        timer = window.setTimeout(() => {
          getCurrentWindow().hide();
        }, 140);
      }
    });
    return () => {
      window.clearTimeout(timer);
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    Promise.all(
      QUALITY.map((q) =>
        getModelStatus(q.value, settings.language).then((s) => [q.value, s.installed] as const),
      ),
    )
      .then((pairs) => {
        if (!cancelled) setInstalled(Object.fromEntries(pairs));
      })
      .catch(console.error);
    getPermissionStatus()
      .then((s) => {
        if (!cancelled) setPermissions(s);
      })
      .catch(console.error);
    return () => {
      cancelled = true;
    };
  }, [settings.model, settings.language]);

  const listening = uiState === 'listening' || uiState === 'finalizing' || uiState === 'inserting';
  const ready =
    permissions?.microphone === 'granted' && permissions?.accessibility === 'granted';

  async function openWindow(view: 'settings' | 'permissions') {
    await invoke('open_settings_window', { view });
  }

  async function setModel(model: WhisperModel) {
    if (model === settings.model || saving) return;
    await save({ ...settings, model });
    if (installed[model] === false) {
      await openWindow('settings');
    }
  }

  async function setLanguage(language: string) {
    if (language === settings.language) return;
    await save({ ...settings, language });
    if (language === 'hinglish') {
      const status = await getModelStatus(settings.model, language);
      if (!status.installed) await openWindow('settings');
    }
  }

  const statusLabel =
    uiState === 'listening'
      ? 'Listening'
      : uiState === 'finalizing'
      ? 'Processing'
      : uiState === 'inserting'
      ? 'Inserting'
      : uiState === 'error'
      ? 'Needs attention'
      : 'Ready';

  return (
    <div className="menu-panel">
      <div className="menu-card">
        <header className="menu-hero">
          <div className={`menu-orb ${listening ? 'live' : ''}`}>
            <img src={myIcon} alt="" />
          </div>
          <div className="menu-hero-copy">
            <div className="menu-hero-top">
              <h1>Voicy</h1>
              <span className={`menu-pill ${listening ? 'live' : ready ? 'ok' : 'wait'}`}>
                <span className="menu-pill-dot" />
                {statusLabel}
              </span>
            </div>
            <p>Hold to dictate, release to insert.</p>
            <div className="menu-keys" aria-label={`Shortcut ${settings.shortcut}`}>
              {shortcutKeys(settings.shortcut).map((key) => (
                <kbd key={key}>{key}</kbd>
              ))}
            </div>
          </div>
        </header>

        <div className={`menu-wave ${listening ? 'live' : ''}`} aria-hidden="true">
          {Array.from({ length: 18 }, (_, i) => (
            <span key={i} style={{ animationDelay: `${i * 0.06}s` }} />
          ))}
        </div>

        {preview && listening && <p className="menu-preview">{preview}</p>}

        <section className="menu-block">
          <div className="menu-block-head">
            <h2>Quality</h2>
            {installed[settings.model] === false && <span className="menu-need">Needs download</span>}
          </div>
          <div className="menu-quality">
            {QUALITY.map((q) => {
              const missing = installed[q.value] === false;
              return (
                <button
                  key={q.value}
                  type="button"
                  className={`menu-quality-btn ${settings.model === q.value ? 'selected' : ''}`}
                  aria-pressed={settings.model === q.value}
                  disabled={saving}
                  onClick={() => setModel(q.value)}
                >
                  <span className="menu-quality-label">{q.label}</span>
                  <span className="menu-quality-hint">{missing ? 'Download' : q.hint}</span>
                </button>
              );
            })}
          </div>
        </section>

        <section className="menu-block">
          <h2>Language</h2>
          <div className="menu-langs" role="listbox" aria-label="Language">
            {LANGUAGES.map((l) => (
              <button
                key={l.value}
                type="button"
                role="option"
                aria-selected={settings.language === l.value}
                className={`menu-lang ${settings.language === l.value ? 'selected' : ''}`}
                onClick={() => setLanguage(l.value)}
              >
                {l.label}
              </button>
            ))}
          </div>
        </section>

        <div className="menu-links">
          <button type="button" className="menu-link" onClick={() => openWindow('settings')}>
            <span className="menu-link-icon" aria-hidden="true">⌥</span>
            <span>
              <strong>Settings</strong>
              <small>Shortcut, overlay, downloads</small>
            </span>
          </button>
          <button type="button" className="menu-link" onClick={() => openWindow('permissions')}>
            <span className={`menu-link-icon ${ready ? 'ok' : ''}`} aria-hidden="true">
              {ready ? '✓' : '!'}
            </span>
            <span>
              <strong>Permissions</strong>
              <small>
                {permissions
                  ? `Mic ${permissions.microphone === 'granted' ? 'on' : 'off'} · Accessibility ${permissions.accessibility === 'granted' ? 'on' : 'off'}`
                  : 'Microphone and Accessibility'}
              </small>
            </span>
          </button>
        </div>

        <button type="button" className="menu-quit" onClick={() => invoke('quit_app')}>
          Quit Voicy
        </button>
        <MadeBy />
      </div>
    </div>
  );
};
