// VoiceKey – hooks/useSettings.ts
// Loads and saves settings via Tauri, with local optimistic update.

import { useEffect, useState, useCallback } from 'react';
import { getSettings, updateSettings } from '../lib/tauri';
import type { Settings } from '../types';
import { DEFAULT_SETTINGS } from '../types';

export interface SettingsHook {
  settings: Settings;
  loading: boolean;
  saving: boolean;
  error: string | null;
  save: (next: Settings) => Promise<void>;
}

export function useSettings(): SettingsHook {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getSettings()
      .then((s) => { setSettings(s); setLoading(false); })
      .catch((e) => { setError(String(e)); setLoading(false); });
  }, []);

  const save = useCallback(async (next: Settings) => {
    setSaving(true);
    setError(null);
    try {
      await updateSettings(next);
      setSettings(next);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }, []);

  return { settings, loading, saving, error, save };
}
