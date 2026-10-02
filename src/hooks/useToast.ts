// VoiceKey – hooks/useToast.ts
// Collects toast messages from Rust error events and local triggers.

import { useState, useCallback, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { ToastMessage, ToastLevel } from '../components/Toast';
import { onError } from '../lib/tauri';

let _nextId = 0;
function nextId() { return `toast-${++_nextId}`; }

const ERROR_LABELS: Record<string, string> = {
  ASR_MODEL_MISSING:  "That speech model isn’t downloaded. Pick it in the menu bar and choose Download.",
  MIC_PERMISSION_DENIED: 'Microphone access denied — check System Settings → Privacy',
  ACCESSIBILITY_DENIED:  'Accessibility denied — enable Voicy in System Settings',
  INSERTION_FAILED:   'Text insertion failed — Accessibility permission required',
  HOTKEY_FAILED:      'Shortcut registration failed — try a different key combination',
};

export function useToast() {
  const [messages, setMessages] = useState<ToastMessage[]>([]);

  const push = useCallback((
    level: ToastLevel,
    title: string,
    body?: string,
    durationMs?: number,
  ) => {
    setMessages((prev) => [
      ...prev,
      { id: nextId(), level, title, body, durationMs },
    ]);
  }, []);

  const dismiss = useCallback((id: string) => {
    setMessages((prev) => prev.filter((m) => m.id !== id));
  }, []);

  // Wire Rust error events → toasts
  useEffect(() => {
    const unlisten = onError((e) => {
      const friendly = ERROR_LABELS[e.code] ?? e.message;
      push('error', friendly, e.code !== friendly ? e.code : undefined, 7000);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [push]);

  // Hotkey registration confirmation
  useEffect(() => {
    const unOk = listen<string>('voicekey://hotkey-ok', (e) => {
      push('success', `Shortcut ready: ${e.payload}`, undefined, 3000);
    });
    const unFail = listen<string>('voicekey://hotkey-failed', (e) => {
      push('error', 'Shortcut registration failed', e.payload, 10000);
    });
    return () => {
      unOk.then((fn) => fn());
      unFail.then((fn) => fn());
    };
  }, [push]);

  return { messages, push, dismiss };
}
