# UI redesign phase 4 — processing stages

Phase 4 introduces a shared, focused processing stage for transcription and
meeting-note generation.

## Implemented

- Previous transcript and meeting-note output is fully hidden while a new result
  is being generated.
- Loose speech fragments animate into aligned text rows to communicate the
  current transformation.
- Transcription and meeting-note generation have separate status copy and
  rotating, restrained humorous lines.
- Determinate progress is shown when chunk or step totals are available.
- Reduced-motion preferences disable all decorative motion.
- The layout changes to a vertical transformation at mobile widths.
- A development-only deterministic preview is available with
  `?preview=processing-stage`; append `&kind=summary` for meeting-note generation.
- In debug builds, the existing `DEV` dock now includes a processing-state
  selector and `待機画面を確認` button. It opens the selected state in a separate,
  resizable 1280 x 720 preview window, matching the overlay preview workflow.

## Evidence

- `processing-stage-1280x720.png` — transcription processing at desktop width.
- `processing-stage-summary-1280x720.png` — meeting-note processing at desktop width.
- `processing-stage-mobile-600x900.png` — responsive transcription state.
- `design-comparison.png` — selected visual direction and implementation in one image.

## Verification

- `pnpm check`
- `pnpm build`
- Browser console and accessibility inspection at 1280 x 720 and 600 x 900.
