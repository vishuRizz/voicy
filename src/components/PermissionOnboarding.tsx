// VoiceKey – components/PermissionOnboarding.tsx
// Guides the user through granting Microphone and Accessibility permissions
// required for VoiceKey to function on macOS.

import React from 'react';
import type { PermissionStatus } from '../types';
import myIcon from '../assets/myicon.png';
import {
  requestMicrophonePermission,
  requestAccessibilityPermission,
} from '../lib/tauri';

interface Props {
  status: PermissionStatus;
  onComplete: () => void;
}

function PermissionRow({
  id,
  icon,
  title,
  description,
  state,
  onGrant,
}: {
  id: string;
  icon: string;
  title: string;
  description: string;
  state: 'granted' | 'denied' | 'notdetermined';
  onGrant: () => void;
}) {
  const isGranted = state === 'granted';
  return (
    <div id={id} className={`permission-row ${state}`}>
      <div className="permission-icon">{icon}</div>
      <div className="permission-info">
        <h3 className="permission-title">{title}</h3>
        <p className="permission-desc">{description}</p>
        {state === 'denied' && (
          <p className="permission-denied-hint">
            Permission was denied. Open System Settings to re-enable.
          </p>
        )}
      </div>
      <div className="permission-action">
        {isGranted ? (
          <span className="permission-granted-badge" aria-label="Permission granted">✓ Granted</span>
        ) : (
          <button
            id={`${id}-grant-btn`}
            className="btn-primary"
            onClick={onGrant}
            aria-label={`Grant ${title} permission`}
          >
            {state === 'denied' ? 'Open Settings' : 'Allow'}
          </button>
        )}
      </div>
    </div>
  );
}

export const PermissionOnboarding: React.FC<Props> = ({ status, onComplete }) => {
  const allGranted =
    status.microphone === 'granted' && status.accessibility === 'granted';

  return (
    <div id="permission-onboarding" className="onboarding-container">
      <div className="onboarding-header">
        <img src={myIcon} alt="VoiceKey" className="onboarding-logo-img" />
        <h1 className="onboarding-title">Welcome to VoiceKey</h1>
        <p className="onboarding-subtitle">
          VoiceKey needs two permissions to work. Your audio is processed
          locally — nothing leaves your device.
        </p>
      </div>

      <div className="permission-list">
        <PermissionRow
          id="permission-microphone"
          icon="🎤"
          title="Microphone"
          description="Required to capture your voice for local transcription."
          state={status.microphone}
          onGrant={requestMicrophonePermission}
        />
        <PermissionRow
          id="permission-accessibility"
          icon="⌨️"
          title="Accessibility"
          description="Required to insert transcribed text into other applications."
          state={status.accessibility}
          onGrant={requestAccessibilityPermission}
        />
      </div>

      <div className="onboarding-footer">
        <p className="privacy-notice">
          🔒 Audio stays on device. No cloud processing. No logging of transcripts.
        </p>
        <button
          id="onboarding-continue-btn"
          className="btn-primary btn-large"
          disabled={!allGranted}
          onClick={onComplete}
          aria-label="Continue to VoiceKey"
        >
          {allGranted ? 'Get Started →' : 'Waiting for permissions…'}
        </button>
      </div>
    </div>
  );
};

export default PermissionOnboarding;
