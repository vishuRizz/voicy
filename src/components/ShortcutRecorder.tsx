// VoiceKey – components/ShortcutRecorder.tsx
// Click to record a global shortcut. Captures modifier + key on keydown.

import React, { useState, useRef, useCallback, useEffect } from 'react';

// Map browser key names → tauri-plugin-global-shortcut format
// (kept for reference; modifier detection uses e.ctrlKey etc. directly)

const MODIFIER_SYMBOLS: Record<string, string> = {
  Super: '⌘',
  Alt:   '⌥',
  Ctrl:  '⌃',
  Shift: '⇧',
};

const BLOCKED_ALONE = new Set(['Meta', 'Alt', 'Control', 'Shift']);

function keyEventToShortcut(e: KeyboardEvent): string | null {
  if (BLOCKED_ALONE.has(e.key)) return null; // modifier-only, wait for the key

  const mods: string[] = [];
  if (e.ctrlKey)  mods.push('Ctrl');
  if (e.altKey)   mods.push('Alt');
  if (e.shiftKey) mods.push('Shift');
  if (e.metaKey)  mods.push('Super');

  if (mods.length === 0) return null; // require at least one modifier

  // Normalise the key name
  let key = e.code; // e.g. "KeyA", "Space", "F1"
  if (key.startsWith('Key')) key = key.slice(3); // "KeyA" → "A"
  else if (key.startsWith('Digit')) key = key.slice(5); // "Digit1" → "1"
  // Space, F1-F12, ArrowUp etc. keep their code name

  return [...mods, key].join('+');
}

function shortcutToDisplay(shortcut: string): string {
  return shortcut
    .split('+')
    .map((part) => MODIFIER_SYMBOLS[part] ?? part)
    .join(' ');
}

interface Props {
  value: string;
  onChange: (shortcut: string) => void;
  id?: string;
}

export const ShortcutRecorder: React.FC<Props> = ({ value, onChange, id }) => {
  const [recording, setRecording] = useState(false);
  const [current, setCurrent] = useState('');
  const ref = useRef<HTMLButtonElement>(null);

  const startRecording = () => {
    setRecording(true);
    setCurrent('');
    ref.current?.focus();
  };

  const stopRecording = useCallback(() => {
    setRecording(false);
    setCurrent('');
  }, []);

  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();

    if (e.key === 'Escape') { stopRecording(); return; }

    const shortcut = keyEventToShortcut(e);
    if (shortcut) {
      onChange(shortcut);
      stopRecording();
    } else {
      // Show partial (just modifiers held so far)
      const held: string[] = [];
      if (e.ctrlKey)  held.push('Ctrl');
      if (e.altKey)   held.push('Alt');
      if (e.shiftKey) held.push('Shift');
      if (e.metaKey)  held.push('Super');
      setCurrent(held.map((m) => MODIFIER_SYMBOLS[m] ?? m).join(' ') + ' …');
    }
  }, [recording, onChange, stopRecording]);

  useEffect(() => {
    if (recording) {
      window.addEventListener('keydown', handleKeyDown, true);
      window.addEventListener('blur', stopRecording);
    }
    return () => {
      window.removeEventListener('keydown', handleKeyDown, true);
      window.removeEventListener('blur', stopRecording);
    };
  }, [recording, handleKeyDown, stopRecording]);

  return (
    <div className="shortcut-recorder">
      <button
        ref={ref}
        id={id}
        type="button"
        className={`shortcut-recorder-btn ${recording ? 'recording' : ''}`}
        onClick={startRecording}
        onBlur={stopRecording}
        aria-label={recording ? 'Press your shortcut keys' : `Current shortcut: ${value}. Click to change.`}
      >
        {recording ? (
          <span className="shortcut-recording-hint">
            {current || 'Press keys…'}
          </span>
        ) : (
          <span className="shortcut-badge">
            {shortcutToDisplay(value) || value}
          </span>
        )}
      </button>
      {recording && (
        <span className="shortcut-recording-note">Press Esc to cancel</span>
      )}
    </div>
  );
};
