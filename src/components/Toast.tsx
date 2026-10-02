// VoiceKey – components/Toast.tsx
// Displays error and info toasts received from Rust events or local triggers.

import React, { useEffect, useState, useCallback } from 'react';

export type ToastLevel = 'error' | 'warning' | 'info' | 'success';

export interface ToastMessage {
  id: string;
  level: ToastLevel;
  title: string;
  body?: string;
  durationMs?: number;
}

interface Props {
  messages: ToastMessage[];
  onDismiss: (id: string) => void;
}

const ICONS: Record<ToastLevel, string> = {
  error:   '✕',
  warning: '⚠',
  info:    'ℹ',
  success: '✓',
};

function ToastItem({ msg, onDismiss }: { msg: ToastMessage; onDismiss: (id: string) => void }) {
  const [exiting, setExiting] = useState(false);

  const dismiss = useCallback(() => {
    setExiting(true);
    setTimeout(() => onDismiss(msg.id), 220);
  }, [msg.id, onDismiss]);

  useEffect(() => {
    const t = setTimeout(dismiss, msg.durationMs ?? 5000);
    return () => clearTimeout(t);
  }, [dismiss, msg.durationMs]);

  return (
    <div
      className={`toast toast--${msg.level} ${exiting ? 'toast--exit' : 'toast--enter'}`}
      role="alert"
      aria-live="assertive"
    >
      <span className="toast__icon">{ICONS[msg.level]}</span>
      <div className="toast__body">
        <span className="toast__title">{msg.title}</span>
        {msg.body && <span className="toast__detail">{msg.body}</span>}
      </div>
      <button
        className="toast__close"
        onClick={dismiss}
        aria-label="Dismiss notification"
      >
        ×
      </button>
    </div>
  );
}

export const ToastContainer: React.FC<Props> = ({ messages, onDismiss }) => {
  if (messages.length === 0) return null;
  return (
    <div id="toast-container" className="toast-container" aria-label="Notifications">
      {messages.map((m) => (
        <ToastItem key={m.id} msg={m} onDismiss={onDismiss} />
      ))}
    </div>
  );
};
