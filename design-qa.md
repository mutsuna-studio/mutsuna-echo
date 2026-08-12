# Design QA — UI redesign phase 3

## Comparison target

- Source visual truth: `docs/ui-redesign/phase-0/selected-direction-desktop.png`
- Implementation screenshot: `docs/ui-redesign/phase-3/phase-3-recording-area-1280x720.png`
- Combined comparison: `docs/ui-redesign/phase-3/design-comparison.png`
- Source pixels: 1487 x 1058.
- Implementation pixels and CSS viewport: 1280 x 720 at device scale 1.
- State: desktop home, idle recording, microphone on, system audio off,
  populated synthetic meeting list.

## Findings

No actionable P0, P1, or P2 finding remains within the phase 3 recording-area
scope.

## Full-view comparison evidence

- Layout and spacing: the recording state, live waveform, central action, and
  three setting groups now follow the selected direction's vertical reading
  order. Dividers connect the settings without introducing a generic card.
- Fonts and typography: existing Noto Sans JP hierarchy remains intact; compact
  status and control labels stay readable without competing with the main page
  heading.
- Colors and tokens: microphone teal, system cyan, silent gray, recording red,
  the phase 1 gradient, and the amber orbit marker all use semantic tokens.
- Image quality and assets: the waveform is a canvas rendering of supplied
  recording levels, not decorative CSS art. Lucide icons remain sharp and
  consistently stroked; there is no placeholder raster imagery.
- Copy and content: ready, start, stop, device, and silence-stop labels describe
  the existing recording workflow directly.

## Focused region comparison evidence

- Spectrum: the desktop hero now reserves 24 logarithmic bands per source from
  80Hz to 12kHz. Microphone occupies the left half and system audio the right;
  both progress from low frequencies at the center to high frequencies outside.
  The compact overlay retains its existing level-history data contract.
- Primary action: the 82px central control is visually isolated by a quiet ring,
  supports keyboard focus, and changes to the recording stop state.
- Audio sources: microphone and system-audio toggles retain their native state,
  device binding, level meter, and disabled behavior while using the approved
  non-searchable `@mutsuna/ui` Select composition.
- Silence stop: the VAD preset remains bound to the existing command and exposes
  the unavailable/preparing messages already supported by the product.

## Responsive and interaction checks

- The desktop composition has no horizontal overflow at the verified 1280px
  viewport.
- Desktop-only additions are hidden at the existing 600px recording breakpoint;
  the established mobile source stack, expanded controls, and semicircle record
  action remain the active composition.
- Accessibility snapshot exposes the recording region/status, labelled record
  action, two checkboxes, three labelled Select trigger buttons, and both level
  meters. Open Selects expose their listbox choices without a text input.
- Disabled system-audio selection remains visibly and semantically disabled.
- Focus indicators, reduced-motion rules, and minimum mobile action sizes remain
  present in the existing component styles.

## Comparison history

- Initial phase 3 capture: `@mutsuna/ui` Select was used as a primitive root, so
  device and VAD values were absent. Classified P1 because recording setup was
  not understandable.
- Fix: composed the library's standard trigger, content, and item primitives,
  restoring full-width normal Selects without editable text fields.
- Second capture: the hero waveform appended its own sample state repeatedly,
  flattening all bars. Classified P1 because it misrepresented level history.
- Fix: sample updates are now keyed only to new level/status input, preserving
  genuine variation and preventing a reactive feedback loop.
- Post-fix evidence: the regenerated combined comparison shows the real level
  history, centered record action, and all three setting groups together.

## Follow-up polish

- P3: the selected concept includes more cyan waveform segments while system
  audio is off. The implementation intentionally reflects actual enabled-source
  data instead of introducing decorative system-audio samples.
- Meeting list density and table metadata remain intentionally deferred to the
  next phase and are not phase 3 acceptance findings.

## Implementation checklist

- Real-time per-source frequency spectrum introduced.
- Central start/stop action implemented.
- Device and VAD choices visibly restored with `@mutsuna/ui` 0.5.0.
- Overlay and mobile recording layouts preserved.
- Type check, production build, licenses, and combined visual QA passed.

final result: pending user verification
