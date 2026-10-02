// VoiceKey – app/App.tsx
// Main application shell: handles onboarding → settings flow and renders the
// listening overlay on top of everything.

import React, { useEffect, useState } from 'react';
import { PermissionOnboarding } from '../components/PermissionOnboarding';
import { SettingsPanel } from '../components/Settings';
import { ListeningOverlay } from '../components/ListeningOverlay';
import { useSession } from '../hooks/useSession';
import { useSettings } from '../hooks/useSettings';
import {
  getPermissionStatus,
  onPermissionsStatus,
  startOnboardingCheck,
} from '../lib/tauri';
import type { PermissionStatus } from '../types';

type AppView = 'onboarding' | 'settings';

const App: React.FC = () => {
  const [view, setView] = useState<AppView>('settings');
  const [permissions, setPermissions] = useState<PermissionStatus>({
    microphone: 'notdetermined',
    accessibility: 'notdetermined',
  });

  const { uiState, preview, finalText, error, cancel } = useSession();
  const { settings, saving, save } = useSettings();

  // ── Permission check on mount ─────────────────────────────────────────────
  useEffect(() => {
    getPermissionStatus().then((s) => {
      setPermissions(s);
      if (s.microphone !== 'granted' || s.accessibility !== 'granted') {
        setView('onboarding');
      }
    });

    // Subscribe to live permission updates from Rust
    const unlisten = onPermissionsStatus((s) => {
      setPermissions(s);
      if (s.microphone === 'granted' && s.accessibility === 'granted') {
        setView('settings');
      }
    });

    startOnboardingCheck();

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // ── Keyboard cancel (Escape) ──────────────────────────────────────────────
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && (uiState === 'listening' || uiState === 'finalizing')) {
        cancel();
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [uiState, cancel]);

  return (
    <div id="app-root" className="app-root">
      {/* Always-present overlay (hidden when idle) */}
      <ListeningOverlay
        uiState={uiState}
        preview={preview}
        finalText={finalText}
        error={error}
        onCancel={cancel}
      />

      {/* Main window content */}
      <div className="main-window">
        <header className="app-header">
          <div className="app-logo">
            <span className="logo-icon">🎙️</span>
            <span className="logo-text">VoiceKey</span>
          </div>
          {view === 'settings' && (
            <div className={`header-status ${uiState}`}>
              <span className={`status-dot ${uiState === 'listening' ? 'pulsing' : ''}`} />
              <span className="status-label">
                {uiState === 'idle'
                  ? `Hold ${settings.shortcut} to dictate`
                  : uiState === 'listening'
                  ? 'Listening…'
                  : uiState === 'finalizing'
                  ? 'Processing…'
                  : uiState === 'inserting'
                  ? 'Inserting…'
                  : 'Error'}
              </span>
            </div>
          )}
        </header>

        <main className="app-main">
          {view === 'onboarding' ? (
            <PermissionOnboarding
              status={permissions}
              onComplete={() => setView('settings')}
            />
          ) : (
            <SettingsPanel settings={settings} saving={saving} onSave={save} />
          )}
        </main>

        <footer className="app-footer">
          <span className="footer-privacy">🔒 Local inference · No cloud · No logging</span>
          <a
            id="open-onboarding-link"
            href="#"
            className="footer-link"
            onClick={(e) => { e.preventDefault(); setView('onboarding'); }}
          >
            Permissions
          </a>
        </footer>
      </div>
    </div>
  );
};

export default App;
