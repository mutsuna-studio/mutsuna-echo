# UI redesign phase 3 — recording area

Phase 3 updates only the desktop recording area. The meeting list, navigation,
recording commands, and compact/mobile composition remain in their existing
phases.

## Implemented

- Added a 24-band real-time spectrum from 80Hz to 8kHz for each native audio
  source, focusing the visualization on the range useful for human speech.
  Low frequencies begin beside the record control and higher frequencies
  extend outward.
- Split the hero visualization into microphone on the left and system audio on
  the right while retaining the existing total-level meters below.
- Promoted the primary record control to the center of the waveform, with an
  explicit active recording status and an active stop state.
- Reorganized microphone, system-audio, and silence-stop controls into one
  visually connected settings row.
- Composed the standard `@mutsuna/ui` 0.5.0 Select trigger, content, and items so
  device and VAD choices have no text input while retaining their bindings.
- Preserved the compact overlay waveform and the mobile recording layout.
- Added a deterministic preview-only spectrum so the fixed visual contract can
  render without adding fake production audio data.

## Deliberately deferred

- Recent-meeting table columns and status treatment
- Additional sidebar destinations or recent-meeting shortcuts
- Meeting detail and settings visual redesign
- Mobile redesign

## Evidence

- `phase-3-recording-area-1280x720.png` — deterministic desktop recording preview
- `design-comparison.png` — selected direction and phase 3 implementation in one image

## Verification

- `pnpm check`
- `pnpm build`
- `pnpm licenses:check`
- Spectrum rendering awaits the user's application-level verification by
  request; no browser automation was run for this follow-up.
