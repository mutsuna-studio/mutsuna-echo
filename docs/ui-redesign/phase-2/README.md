# UI redesign phase 2 — desktop shell

Phase 2 updates the desktop application shell while keeping recording,
transcription, meeting selection, settings navigation, and mobile behavior
unchanged.

## Implemented

- Rebalanced the desktop shell to an 18rem sidebar, matching the selected
  direction's sidebar-to-content proportion.
- Increased desktop sidebar breathing room and clarified the brand, selected
  recording section, and settings hierarchy.
- Removed the redundant desktop breadcrumb bar from both the visual layout and
  keyboard order. The existing compact/mobile header remains available at
  780px and below.
- Added a content-owned page heading, supporting copy, and localized date.
- Made the main surface flat and continuous, with a quiet sidebar separator and
  the phase 1 canvas treatment visible behind the content.
- Aligned the project with `@mutsuna/ui` 0.5.0, whose shell API is used by the
  implementation.

## Deliberately deferred

- Central live waveform and large recording focal control
- Recording-source control composition
- Recent-meeting table columns and status treatment
- Additional sidebar destinations or duplicate recent-meeting content
- Meeting detail and settings visual redesign
- Mobile redesign

## Evidence

- `phase-2-desktop-shell-1280x720.png` — deterministic desktop shell preview
- `design-comparison.png` — selected direction and phase 2 shell in one image

## Verification

- `pnpm check`
- `pnpm build`
- `pnpm licenses:check`
- In-app browser DOM, focus, overflow, and computed shell-width checks
