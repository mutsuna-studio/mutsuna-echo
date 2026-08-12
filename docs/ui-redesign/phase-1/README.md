# UI redesign phase 1 — design foundation

Phase 1 applies the visual foundation selected in phase 0 without changing the
screen layout, information architecture, or behavior.

## Implemented

- Added a pale mist canvas, deep teal primary color, cyan system-audio color,
  amber warning/accent color, and recording red as shared CSS tokens.
- Added shared surface, border, focus-ring, shadow, radius, spacing, and audio
  semantic tokens.
- Added restrained shared gradients for the canvas glow, active navigation, and
  recording start control. Recording-active red remains unambiguous and solid.
- Bundled `Noto Sans JP Variable` for Japanese UI text and retained
  `Inter Variable` for numeric data.
- Replaced component-local colors in recording controls, meters, history, and
  mobile recording states with semantic tokens.
- Updated the application and overlay theme primary color to the selected teal.
- Updated third-party license artifacts for the bundled font.

## Deliberately deferred

- Sidebar and header restructuring
- The large central waveform/start control
- Recording-source control layout
- Recent-meeting table and status redesign
- Responsive layout changes

Those items remain phase 2 and later work so that this phase can be reviewed as
a visual-foundation change only.

## Evidence

- `phase-1-desktop-home-1280x720.png` — deterministic desktop preview
- `design-comparison.png` — selected direction and phase 1 implementation in a
  single comparison image, including the final gradient treatment

## Verification

- `pnpm check`
- `pnpm build`
- `pnpm licenses:check`
- In-app browser DOM, local-font load, computed-token, and keyboard-focus checks
