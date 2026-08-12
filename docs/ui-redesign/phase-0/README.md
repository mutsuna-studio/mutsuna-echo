# UI redesign phase 0 baseline

Created: 2026-08-12

## Status and scope

Phase 0 fixes the visual target, captures the current UI, and defines the boundary for later work. It does not change the production UI, recording behavior, navigation, stored data, or native credential handling.

The selected direction is the second generated concept. Treat it as the source of truth for desktop visual hierarchy, color mood, typography, spacing, and the audio-focused recording area. It is not approval to add routes, product features, account UI, or data that do not exist in the current application.

## Visual references

| File | Size | Purpose |
| --- | --- | --- |
| `selected-direction-desktop.png` | 1487 x 1058 | Selected redesign direction and desktop visual target |
| `baseline-desktop-home-1440x1024.png` | 1440 x 1024 | Current desktop home at the main reference viewport |
| `baseline-compact-home-780x1024.png` | 780 x 1024 | Current compact desktop layout near the shell breakpoint |
| `baseline-mobile-webview-600x900.png` | 600 x 900 | Current responsive WebView layout at the mobile breakpoint |
| `baseline-android-home.png` | 1080 x 2424 | Current Android home captured on a physical device |
| `baseline-desktop-settings.png` | 1280 x 720 | Current desktop settings styling for later consistency work |

The desktop and responsive baseline captures use deterministic development-only data. The Android image is the existing verified physical-device capture.

## Selected direction: approved interpretation

Preserve these characteristics from the selected concept:

- pale blue-gray base surface and restrained teal/cyan audio accents;
- strong Japanese page heading with calm supporting copy;
- a horizontal, audio-native recording focal area;
- microphone, system audio, and silence detection grouped as recording sources;
- a flat recent-meetings list with lightweight row separators;
- one unmistakable recording action and quiet secondary actions;
- generous whitespace, readable type, and restrained elevation.

Do not infer these items from the generated concept:

- new routes such as a separate transcription-history or imported-files product area;
- user accounts, billing, analytics, storage quotas, or upgrade prompts;
- new meeting metadata that the application does not store;
- a decorative fake waveform. A later implementation must use real recording levels;
- a mobile redesign. The current verified Android recording control stays unchanged until a mobile visual is separately approved.

## Current implementation map

| Visible area | Current implementation | Later redesign boundary |
| --- | --- | --- |
| Application shell and page header | `src/App.svelte`, `AdminShellFrame` | Shell spacing, header treatment, responsive behavior |
| Sidebar and settings entry | `src/lib/components/AppSidebar.svelte` | Brand treatment, selected state, navigation density |
| Recording and meeting home | `src/lib/components/MeetingHome.svelte` | Page composition and recent-meeting hierarchy |
| Recording sources and action | `src/lib/components/RecordingPanel.svelte` | Recording focal area, source controls, state presentation |
| Live audio history | `src/lib/components/AudioLevelWaveform.svelte` | Wider real-time waveform and source colors |
| Meeting detail | `src/lib/components/MeetingWorkspace.svelte` | Deferred until the desktop home direction is accepted |
| Transcript, notes, and playback | `TranscriptView.svelte`, `MeetingSummary.svelte`, `AudioPlayer.svelte` | Deferred until the home direction is accepted |
| Shared visual tokens | `src/app.css`, `createTheme` in `src/App.svelte` | Color, type, spacing, radii, focus, elevation |

## State checklist for later visual acceptance

Each state must retain its existing behavior and be captured at the same viewport before a phase is accepted.

### Home and recording

- idle, microphone enabled, system audio disabled;
- idle with both sources enabled;
- empty meeting list;
- populated meeting list;
- recording starting, active, finalizing, and completed;
- recoverable interrupted recording;
- unavailable input source, permission failure, and recording error;
- active transcription, diarization, formatting, or summary status on a meeting row;
- reduced-motion mode.

### Responsive surfaces

- desktop at 1440 x 1024;
- compact desktop at 780 x 1024;
- WebView mobile breakpoint at 600 x 900;
- Android portrait on an API 29+ device or emulator;
- safe-area handling and the existing Android back behavior.

### Interaction and accessibility

- keyboard traversal follows the visual order;
- every control has a visible focus state;
- recording status is not conveyed by color alone;
- Japanese labels do not truncate at supported sizes;
- 200% zoom does not hide the primary recording action;
- motion respects `prefers-reduced-motion`.

## Development-only baseline preview

Run the preview server and open the fixed URL:

```powershell
pnpm exec vite dev --host 127.0.0.1 --port 4173 --strictPort
```

```text
http://127.0.0.1:4173/?preview=meeting-home-baseline
```

The preview is selected only when `import.meta.env.DEV` is true. It bypasses native recording calls, uses synthetic meeting names and file sizes, and must never contain secrets or copied customer data.

## Existing worktree protection

The following files had user changes before or while phase 0 and are outside this phase's scope. They must not be reset or replaced during the redesign:

- `src-tauri/src/commands/usage.rs`
- `src-tauri/src/transcript_store.rs`
- `src/App.svelte`
- `src/app.css`
- `src/lib/components/AppUpdateManager.svelte`
- `src/lib/components/MeetingHome.svelte`
- `src/lib/components/MeetingSummary.svelte`
- `src/lib/components/MeetingWorkspace.svelte`
- `src/lib/components/RecordingMode.svelte`
- `src/lib/components/ThirdPartyLicenses.svelte`
- `src/lib/components/TranscriptView.svelte`
- `src/lib/types/summary.ts`
- `src/lib/components/SummarySourceSheet.svelte`

Future phases should remain reviewable as small diffs and stop for screenshot approval before moving to the next phase.

## Phase 0 completion criteria

- the selected desktop direction is stored in the repository;
- current desktop, compact, WebView mobile, Android, and settings references are stored;
- the development-only baseline preview renders without native API calls;
- the preview has no browser console errors or warnings;
- `pnpm check` and `pnpm build` pass;
- no production UI styling or application behavior is changed.
