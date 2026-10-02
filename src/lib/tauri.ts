// VoiceKey – lib/tauri.ts
// Typed wrappers around Tauri commands and event listeners.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  Settings,
  ModelStatus,
  PermissionStatus,
  StateEvent,
  PreviewEvent,
  FinalEvent,
  ErrorEvent,
} from '../types';

// ── Commands ─────────────────────────────────────────────────────────────────

export const getSettings = (): Promise<Settings> =>
  invoke<Settings>('get_settings');

export const updateSettings = (settings: Settings): Promise<void> =>
  invoke<void>('update_settings', { settings });

export const cancelSession = (): Promise<void> =>
  invoke<void>('cancel_session');

export const getModelStatus = (model?: string, language?: string): Promise<ModelStatus> =>
  invoke<ModelStatus>('get_model_status', {
    ...(model ? { model } : {}),
    ...(language ? { language } : {}),
  });

export const startOnboardingCheck = (): Promise<void> =>
  invoke<void>('start_onboarding_check');

export const getPermissionStatus = (): Promise<PermissionStatus> =>
  invoke<PermissionStatus>('get_permission_status');

export const requestMicrophonePermission = (): Promise<void> =>
  invoke<void>('request_microphone_permission');

export const requestAccessibilityPermission = (): Promise<void> =>
  invoke<void>('request_accessibility_permission');

// ── Event listeners ───────────────────────────────────────────────────────────

export const onSessionState = (cb: (e: StateEvent) => void): Promise<UnlistenFn> =>
  listen<StateEvent>('session://state', (event) => cb(event.payload));

export const onPreview = (cb: (e: PreviewEvent) => void): Promise<UnlistenFn> =>
  listen<PreviewEvent>('session://preview', (event) => cb(event.payload));

export const onFinal = (cb: (e: FinalEvent) => void): Promise<UnlistenFn> =>
  listen<FinalEvent>('session://final', (event) => cb(event.payload));

export const onError = (cb: (e: ErrorEvent) => void): Promise<UnlistenFn> =>
  listen<ErrorEvent>('session://error', (event) => cb(event.payload));

export const onPermissionsStatus = (cb: (s: PermissionStatus) => void): Promise<UnlistenFn> =>
  listen<PermissionStatus>('permissions://status', (event) => cb(event.payload));
