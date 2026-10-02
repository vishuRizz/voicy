# VoiceKey --- Product Requirements Document (PRD) & Technical Requirements Document (TRD)

**Version:** 1.0\
**Date:** 2 October 2026\
**Status:** MVP specification\
**Target platforms:** macOS first, Windows second\
**Proposed stack:** Tauri 2, React, TypeScript, Rust, `whisper.cpp`

------------------------------------------------------------------------

# Part I --- Product Requirements Document (PRD)

## 1. Product overview

**VoiceKey** is a lightweight desktop dictation app that lets a user
speak into almost any text field on their computer. The user holds a
configurable global hotkey to record, sees a live transcription preview
while speaking, and releases the key to finalize and insert the text
into the previously focused application.

Speech recognition runs locally using Whisper. By default, audio is kept
in memory and discarded after transcription; the MVP does not send audio
or transcripts to a cloud service.

## 2. Problem statement

Typing is not always the fastest or most comfortable way to enter text.
Existing dictation workflows can require switching apps, clicking a
microphone button, or relying on cloud processing. VoiceKey aims to make
dictation available through a consistent hold-to-talk interaction with
local processing and a minimal interruption to the user's workflow.

## 3. Goals

-   Start and stop dictation using a configurable global hold key.
-   Capture microphone audio while the key is held.
-   Show a provisional, live transcription preview during recording.
-   On release, finalize the utterance and insert the final text into
    the app that had focus before dictation.
-   Run speech recognition locally.
-   Provide clear recording, processing, error, and permission states.
-   Support macOS in the first release and keep the architecture ready
    for Windows.

## 4. Non-goals for MVP

-   Cloud transcription or account-based sync.
-   Conversation, summarization, or AI rewriting features.
-   Custom vocabulary training or user-specific model fine-tuning.
-   Linux support.
-   Guaranteed insertion into every protected or privileged
    field/application.
-   Automatic punctuation/style rewriting beyond the selected Whisper
    model's normal output.

## 5. Target users and use cases

**Primary users:** people who frequently write messages, notes, prompts,
emails, documents, or code comments and want quick system-wide
dictation.

**Core use cases:** 1. Hold the hotkey and dictate a short message into
a chat app. 2. Dictate a paragraph into a document editor while watching
the preview. 3. Release the key and have the finalized transcript
inserted at the original cursor. 4. Cancel a recording without inserting
text. 5. Change the hotkey, language, or local model in settings.

## 6. User journey

1.  User installs VoiceKey and completes microphone and required
    system-permission onboarding.
2.  User chooses or confirms a hold-to-talk shortcut.
3.  User focuses a text field in another application.
4.  User presses and holds the shortcut.
5.  VoiceKey begins capturing audio and displays a compact listening
    overlay.
6.  A provisional transcript updates while the user speaks.
7.  User releases the shortcut.
8.  VoiceKey finalizes transcription and inserts the final text into the
    previously focused field.
9.  The overlay returns to idle and the audio buffer is discarded.

## 7. Functional requirements

  -----------------------------------------------------------------------
  ID                      Requirement             Priority
  ----------------------- ----------------------- -----------------------
  FR-01                   Register a configurable P0
                          global hold-to-talk     
                          shortcut.               

  FR-02                   Start microphone        P0
                          capture on key-down and 
                          stop on key-up.         

  FR-03                   Show a visible          P0
                          listening state while   
                          recording.              

  FR-04                   Show rolling,           P0
                          provisional             
                          transcription updates   
                          during recording.       

  FR-05                   Run live and final      P0
                          speech recognition      
                          locally.                

  FR-06                   Finalize the full       P0
                          utterance after         
                          release; do not insert  
                          provisional text.       

  FR-07                   Insert final text into  P0
                          the previously focused  
                          application when        
                          supported.              

  FR-08                   Provide a cancel action P0
                          that discards the       
                          current utterance.      

  FR-09                   Explain and detect      P0
                          missing                 
                          microphone/system       
                          permissions.            

  FR-10                   Allow language and      P1
                          model selection from    
                          supported local         
                          options.                

  FR-11                   Provide a menu-bar/tray P1
                          entry for settings and  
                          quit.                   

  FR-12                   Provide clipboard       P1
                          fallback when direct    
                          text insertion fails.   
  -----------------------------------------------------------------------

## 8. UX and interaction requirements

-   **Idle:** no recording indicator; shortcut is armed.
-   **Listening:** compact, non-intrusive overlay with a microphone
    indicator and live preview.
-   **Finalizing:** indicate that the final transcript is being
    computed.
-   **Inserting:** brief status while text is being delivered to the
    target app.
-   **Error:** concise explanation and an actionable recovery step.
-   The overlay should avoid taking focus from the target application.
-   Live text is explicitly provisional and may change as more audio
    context becomes available.
-   The app must never paste partial preview text automatically.
-   Include an obvious cancel gesture or shortcut and a clear indication
    when recording is active.

## 9. Success metrics

Measure on a documented reference device and representative audio:

-   Time from key-down to visible recording state.
-   Delay between speech and preview updates.
-   Time from key-up to final insertion.
-   Successful hotkey activation rate.
-   Successful text insertion rate across a defined test matrix.
-   Crash-free sessions and transcription failure rate.
-   Permission-onboarding completion rate.

Set numeric targets after an initial performance spike; do not promise
identical latency on all hardware.

## 10. MVP acceptance criteria

-   User can configure and use a global hold-to-talk key on macOS.
-   Recording starts on key-down and stops on key-up without requiring
    the VoiceKey window to be active.
-   A live provisional transcript appears and updates during a
    recording.
-   Releasing the key produces a finalized transcript from the full
    utterance.
-   Final text is inserted into a supported target field without
    stealing focus during recording.
-   Canceling does not insert text.
-   Recognition works with network access disabled after the model is
    installed.
-   The user can understand and resolve missing permissions.
-   Audio is not persisted by default and no cloud fallback occurs.

------------------------------------------------------------------------

# Part II --- Technical Requirements Document (TRD)

## 11. Recommended technology stack

  -----------------------------------------------------------------------
  Layer                   Technology              Responsibility
  ----------------------- ----------------------- -----------------------
  Desktop shell           Tauri 2                 App lifecycle, windows,
                                                  packaging, native
                                                  integration boundary

  UI                      React + TypeScript      Settings, onboarding,
                                                  compact overlay

  Native/core             Rust                    State machine, hotkey
                                                  handling, audio
                                                  orchestration,
                                                  insertion

  Speech recognition      `whisper.cpp`           Local live and final
                                                  inference

  Audio capture           Rust audio library /    Microphone input and
                          platform audio APIs     PCM buffering

  Persistence             Tauri plugin/store or   User settings and model
                          small config file       metadata

  Build                   Tauri tooling +         Development builds and
                          platform CI             installers
  -----------------------------------------------------------------------

**Why this stack:** Tauri keeps the interface web-based while Rust
handles latency-sensitive and OS-specific behavior. The UI and most
product logic can be shared, while hotkeys, permissions, and text
insertion are implemented behind platform adapters. A Python/PySide6
prototype may be faster for validating the interaction, but the intended
product architecture is Tauri.

## 12. System architecture

``` text
┌───────────────────────────────────────────┐
│              React UI (Webview)           │
│ Settings · Onboarding · Listening Overlay │
└─────────────────────┬─────────────────────┘
                      │ Tauri commands/events
┌─────────────────────▼─────────────────────┐
│                Rust Core                  │
│ App State · Session Coordinator           │
│ Hotkey Manager · Audio Manager             │
│ Transcription Coordinator · Text Insertion │
└───────┬──────────────┬──────────────┬──────┘
        │              │              │
   Global Hotkey   Microphone     Local ASR
   OS Adapter      Audio Input    whisper.cpp
        │              │              │
        └──────────────┴──────────────┘
                       │
                Text Insertion
             Accessibility / Clipboard
```

### Core components

-   **App state:** owns the current state and session identifier.
-   **Hotkey manager:** registers the chosen shortcut and emits
    key-down/key-up events.
-   **Audio manager:** opens the microphone, converts/resamples audio to
    the model's expected format, and maintains an in-memory buffer.
-   **Transcription coordinator:** schedules rolling preview inference
    and final full-utterance inference.
-   **Insertion manager:** restores or targets the prior focused app and
    delivers final text using supported OS mechanisms.
-   **Settings manager:** persists shortcut, language, model choice, and
    user preferences.
-   **Platform adapters:** isolate macOS and Windows-specific APIs and
    permission behavior.

## 13. Recording state machine

``` text
IDLE
  │ hotkey down
  ▼
LISTENING
  │ hotkey up                 │ cancel / capture error
  ▼                           ▼
FINALIZING                  IDLE / ERROR
  │ final transcript ready
  ▼
INSERTING
  │ success / fallback
  ▼
IDLE
```

Rules: - Ignore repeated key-down events while already listening. - A
session has a unique ID; late inference results from an old session must
be ignored. - On release, stop capture and schedule final inference over
the complete utterance. - On cancel, stop capture, clear the buffer, and
discard pending results. - Handle microphone interruption and app
shutdown without leaving the microphone active.

## 14. Audio capture and buffering

-   Capture mono PCM audio at a rate supported by the chosen Whisper
    pipeline; resample as needed.
-   Keep audio in memory for the active session.
-   Use a bounded queue between capture and inference to avoid unbounded
    memory growth.
-   Detect device loss, permission denial, and capture initialization
    errors.
-   Avoid writing raw audio to disk unless a future, explicit opt-in
    feature is designed.
-   Keep capture and inference off the UI thread.

## 15. Live transcription strategy

Use rolling-window inference for the preview:

1.  Accumulate audio during the active session.
2.  At a configurable interval, run inference on a recent window with a
    small amount of preceding context.
3.  Replace or reconcile the provisional preview with the latest result.
4.  On key release, run a final pass over the complete utterance.
5.  Display and insert only the final result.

Initial tuning range: - Context window: approximately 4--6 seconds. -
Preview refresh: approximately every 1--2 seconds. - These are starting
points for benchmarking, not fixed performance guarantees.

Implementation considerations: - Avoid running overlapping inference
jobs without limits. - Prefer the newest useful preview and discard
stale updates. - Mark preview text as provisional. - Evaluate
partial-result stability, latency, CPU usage, memory use, and
transcription quality on reference hardware. - Final inference may
differ from the live preview; the final result is authoritative.

## 16. Local model management

-   Bundle or download a supported local model with clear license and
    size information.
-   Verify model integrity before loading.
-   Show download/install progress if models are not bundled.
-   Store model files in an application-managed directory.
-   Do not silently switch to cloud inference if local inference fails.
-   Provide a clear error if the selected model is unavailable or
    incompatible.
-   Benchmark model size, accuracy, startup time, and CPU/GPU behavior
    on supported devices.

## 17. Text insertion and focus handling

Text insertion is the most platform-sensitive part of the product.

Preferred flow: 1. Record the target application's identity/focus
context before showing UI that could affect focus. 2. Keep the overlay
non-activating where the OS allows. 3. After finalization, attempt
supported direct text insertion. 4. If direct insertion is unavailable
or fails, use a controlled clipboard-paste fallback. 5. Restore
clipboard contents when feasible and safe; document cases where this
cannot be guaranteed. 6. Never claim universal compatibility.

Known limitations: - Secure password fields and protected system UI may
reject synthetic input. - Elevated applications, remote desktop
sessions, games, and some terminal/editor configurations may behave
differently. - Accessibility permissions and OS privacy controls can
prevent insertion. - Focus may change while the user is speaking; define
and test expected behavior.

## 18. Platform requirements

### macOS --- first release

-   Microphone permission and clear onboarding.
-   Required Accessibility and/or Input Monitoring permissions for the
    chosen global-hotkey and insertion implementation.
-   Menu-bar presence and a non-intrusive overlay.
-   Test on supported macOS versions and both Apple Silicon and Intel
    only if both are in scope.
-   Validate behavior after permission changes and app restarts.

### Windows --- second release

-   Microphone privacy permission handling.
-   Global shortcut registration and key-up detection.
-   Text insertion using supported input APIs, with clipboard fallback.
-   Test standard and elevated apps, multiple displays, sleep/wake, and
    device changes.
-   Keep Windows-specific behavior behind the same platform interface.

Linux is outside the MVP.

## 19. Privacy and security

-   Default to local-only inference.
-   No audio or transcript telemetry by default.
-   No cloud fallback.
-   Audio remains in memory and is discarded after completion or
    cancellation.
-   Persist only settings and necessary model metadata.
-   Explain microphone and accessibility permissions in plain language.
-   Do not log transcript content, raw audio, or sensitive text.
-   Provide a visible recording indicator and a reliable cancel action.
-   Review dependencies, model licenses, update integrity, and
    application signing before release.

## 20. Suggested project structure

``` text
voicekey/
├── src/
│   ├── app/
│   ├── components/
│   │   ├── ListeningOverlay.tsx
│   │   ├── Settings.tsx
│   │   └── PermissionOnboarding.tsx
│   ├── hooks/
│   ├── lib/
│   └── types/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs
│   │   ├── app_state.rs
│   │   ├── hotkey.rs
│   │   ├── audio.rs
│   │   ├── transcription.rs
│   │   ├── insertion.rs
│   │   ├── settings.rs
│   │   └── platform/
│   │       ├── macos.rs
│   │       └── windows.rs
│   ├── capabilities/
│   └── tauri.conf.json
├── models/
├── scripts/
└── README.md
```

## 21. Frontend/native interface

Illustrative event and command contract:

**Rust → UI events** - `session://state`: session state and session
ID. - `session://preview`: provisional transcript and session ID. -
`session://final`: finalized transcript and session ID. -
`session://error`: structured error code and user-facing message. -
`permissions://status`: microphone and platform permission state.

**UI → Rust commands** - `get_settings` - `update_settings` -
`start_onboarding_check` - `cancel_session` - `get_model_status` -
`install_model`

The native core remains authoritative for recording state. UI events are
presentation updates, not permission to start or stop the microphone.

## 22. Error handling

Use structured error codes, for example: - `MIC_PERMISSION_DENIED` -
`MIC_DEVICE_UNAVAILABLE` - `HOTKEY_REGISTRATION_FAILED` -
`ASR_MODEL_MISSING` - `ASR_INFERENCE_FAILED` -
`INSERTION_PERMISSION_MISSING` - `INSERTION_UNSUPPORTED` -
`SESSION_CANCELLED`

Each error should include a short explanation, recovery action, and
diagnostic details that exclude user audio and transcript content.

## 23. Performance and reliability

Benchmark and report: - Hotkey-to-capture startup delay. - Preview
latency and update frequency. - Finalization time after release. - CPU
and memory use during recording and idle. - Model load time and
application startup time. - Insertion success by app category.

Do not set universal guarantees before measuring across the supported
hardware matrix. Prioritize responsiveness without allowing inference to
starve audio capture or block the UI.

## 24. Testing plan

**Unit tests** - State transitions and invalid event handling. - Session
ID and stale-result handling. - Settings validation. - Audio
chunking/resampling boundaries. - Preview reconciliation and
final-result precedence.

**Integration tests** - Microphone capture and device interruption. -
Local model loading and inference. - Rolling preview update behavior. -
Full-utterance finalization. - Cancellation and buffer disposal.

**Platform tests** - Global shortcut key-down/key-up behavior. - Overlay
focus behavior. - Insertion into a defined app matrix. - Permission
denial, revocation, and recovery. - Clipboard fallback and clipboard
restoration behavior.

**Privacy tests** - Verify no network requests are made for
transcription. - Verify raw audio is not persisted by default. - Verify
logs exclude transcript and audio content.

## 25. Delivery plan

### Phase 0 --- Feasibility spike

-   Validate macOS global hold shortcut and key-up detection.
-   Validate non-activating overlay behavior.
-   Validate text insertion into a small target-app matrix.
-   Confirm local `whisper.cpp` integration and model performance.

### Phase 1 --- Core MVP

-   Implement state machine, audio capture, final inference, and
    insertion.
-   Add settings, onboarding, error handling, and cancellation.
-   Add local model installation/selection.

### Phase 2 --- Live preview

-   Add rolling-window inference and provisional UI.
-   Benchmark latency, resource use, and preview stability.
-   Tune window size and refresh interval.

### Phase 3 --- Release hardening

-   Package, sign, and test macOS builds.
-   Complete privacy and permission review.
-   Add diagnostics that do not collect sensitive content.
-   Prepare user documentation and known-limitations page.

### Phase 4 --- Windows port

-   Implement Windows hotkey, audio, permission, and insertion adapters.
-   Run the platform test matrix.
-   Keep shared product behavior consistent while documenting
    OS-specific differences.

## 26. Key risks and mitigations

  -----------------------------------------------------------------------
  Risk                    Impact                  Mitigation
  ----------------------- ----------------------- -----------------------
  Focus changes or        Text goes to wrong      Capture target context
  overlay steals focus    place                   early; non-activating
                                                  overlay; test
                                                  extensively

  OS blocks synthetic     Insertion fails         Permission onboarding;
  input                                           clipboard fallback;
                                                  disclose limitations

  Live inference is too   Preview feels delayed   Benchmark early; tune
  slow                                            model/window; limit
                                                  concurrent jobs

  Preview text changes    Distracting UX          Label provisional;
  frequently                                      reconcile updates;
                                                  final pass
                                                  authoritative

  Model download is large Onboarding friction     Explain size; show
                                                  progress; allow
                                                  supported model choice

  Platform APIs differ    Cross-platform drift    Rust platform adapters
                                                  and shared interface

  Sensitive text appears  Privacy exposure        No transcript logging;
  in logs                                         privacy-focused
                                                  diagnostics
  -----------------------------------------------------------------------

## 27. Open decisions

-   Exact default shortcut and conflict-resolution UX.
-   Initial language support and automatic language detection behavior.
-   Default model size and distribution method.
-   Whether punctuation/capitalization settings are exposed in MVP.
-   Exact macOS version floor and hardware support matrix.
-   Clipboard restoration behavior when another app modifies the
    clipboard during transcription.
-   Whether users can configure a maximum recording duration.

------------------------------------------------------------------------

**Product principle:** VoiceKey should feel like a dependable system
utility: fast to invoke, clear when active, private by default, and
honest about the limits of cross-application text insertion.
