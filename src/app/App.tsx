// VoiceKey – app/App.tsx
// Main application shell: handles onboarding → settings flow and renders the
// listening overlay on top of everything.

import React, { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { PermissionOnboarding } from '../components/PermissionOnboarding';
import { SettingsPanel } from '../components/Settings';
import { ListeningOverlay } from '../components/ListeningOverlay';
import { MenuBarPanel } from '../components/MenuBarPanel';
import { ToastContainer } from '../components/Toast';
import { useSession } from '../hooks/useSession';
import { useSettings } from '../hooks/useSettings';
import { useToast } from '../hooks/useToast';
import {
  getPermissionStatus,
  onPermissionsStatus,
  startOnboardingCheck,
} from '../lib/tauri';
import type { PermissionStatus } from '../types';
import myIcon from '../assets/myicon.png';

type AppView = 'onboarding' | 'settings';

function useWindowLabel(): string | null {
  const [label, setLabel] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    import('@tauri-apps/api/window')
      .then(({ getCurrentWindow }) => {
        if (live) setLabel(getCurrentWindow().label);
      })
      .catch(() => {
        if (live) setLabel('main');
      });
    return () => {
      live = false;
    };
  }, []);
  return label;
}

const MainShell: React.FC = () => {
  const [view, setView] = useState<AppView>('settings');
  const [permissions, setPermissions] = useState<PermissionStatus>({
    microphone: 'notdetermined',
    accessibility: 'notdetermined',
  });

  const { uiState, preview, finalText, error, cancel } = useSession();
  const { settings, saving, save } = useSettings();
  const { messages, push, dismiss } = useToast();

  // ── Permission check on mount ─────────────────────────────────────────────
  useEffect(() => {
    getPermissionStatus().then((s) => {
      setPermissions(s);
      if (s.microphone !== 'granted' || s.accessibility !== 'granted') {
        setView('onboarding');
      }
    });

    const unlisten = onPermissionsStatus((s) => {
      setPermissions(s);
      if (s.microphone === 'granted' && s.accessibility === 'granted') {
        setView('settings');
      }
    });

    startOnboardingCheck();

    const unview = listen<string>('voicekey://show-view', (e) => {
      setView(e.payload === 'permissions' ? 'onboarding' : 'settings');
    });

    // Re-check whenever user switches back (e.g. returns from System Settings).
    const recheck = () => {
      getPermissionStatus().then((s) => {
        setPermissions(s);
        if (s.microphone === 'granted' && s.accessibility === 'granted') {
          setView('settings');
        }
      });
    };
    window.addEventListener('focus', recheck);
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible') recheck();
    });

    return () => {
      unlisten.then((fn) => fn());
      unview.then((fn) => fn());
      window.removeEventListener('focus', recheck);
    };
  }, []);

  // ── Surface session errors as toasts ─────────────────────────────────────
  useEffect(() => {
    if (error) {
      push('error', error.message ?? 'An error occurred', error.code, 7000);
    }
  }, [error, push]);

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
      {/* Global toast notifications */}
      <ToastContainer messages={messages} onDismiss={dismiss} />

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
            <img src={myIcon} alt="Voicy" className="app-logo-img" />
            <span className="logo-text">Voicy</span>
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

const App: React.FC = () => {
  const label = useWindowLabel();
  if (label === null) return null;
  if (label === 'menu') return <MenuBarPanel />;
  return <MainShell />;
};

export default App;
