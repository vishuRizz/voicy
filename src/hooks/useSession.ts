// VoiceKey – hooks/useSession.ts
// Subscribes to all Rust→UI session events and exposes clean React state.

import { useEffect, useState, useCallback } from 'react';
import type { UnlistenFn } from '@tauri-apps/api/event';
import {
  onSessionState,
  onPreview,
  onFinal,
  onError,
  cancelSession,
} from '../lib/tauri';
import type { ErrorEvent } from '../types';

export type UiState = 'idle' | 'listening' | 'finalizing' | 'inserting' | 'error';

export interface SessionHook {
  uiState: UiState;
  preview: string;
  finalText: string | null;
  error: ErrorEvent | null;
  sessionId: string | null;
  cancel: () => Promise<void>;
}

export function useSession(): SessionHook {
  const [uiState, setUiState] = useState<UiState>('idle');
  const [preview, setPreview] = useState('');
  const [finalText, setFinalText] = useState<string | null>(null);
  const [error, setError] = useState<ErrorEvent | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(null);

  useEffect(() => {
    const unlisteners: Promise<UnlistenFn>[] = [];

    unlisteners.push(
      onSessionState((e) => {
        setSessionId(e.session_id);
        const s = e.state.toUpperCase();
        if (s === 'IDLE') { setUiState('idle'); setPreview(''); setFinalText(null); setError(null); }
        else if (s === 'LISTENING') { setUiState('listening'); setFinalText(null); setError(null); }
        else if (s === 'FINALIZING') setUiState('finalizing');
        else if (s === 'INSERTING') setUiState('inserting');
      }),
    );

    unlisteners.push(
      onPreview((e) => {
        if (e.is_provisional) setPreview(e.text);
      }),
    );

    unlisteners.push(
      onFinal((e) => {
        setFinalText(e.text);
      }),
    );

    unlisteners.push(
      onError((e) => {
        setError(e);
        setUiState('error');
      }),
    );

    return () => {
      unlisteners.forEach((p) => p.then((fn) => fn()));
    };
  }, []);

  const cancel = useCallback(async () => {
    await cancelSession();
  }, []);

  return { uiState, preview, finalText, error, sessionId, cancel };
}
