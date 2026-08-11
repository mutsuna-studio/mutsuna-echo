# Design QA

## Evidence

- Visual source of truth: `C:\Users\taich.000\.codex\generated_images\019feed0-56cd-7d41-b80c-9ff0a981ab84\exec-903bd854-690d-4ba1-9ee7-0deaeacce831.png`
- Android implementation capture: `E:\MutsunaJP\mutsuna-echo\.local-notes\unified-home-android-final-v3.png`
- Side-by-side comparison: `E:\MutsunaJP\mutsuna-echo\.local-notes\design-qa-comparison.png`
- Source dimensions: 1536 x 1024 px (desktop and mobile concept board)
- Implementation dimensions: 1080 x 2424 px (Pixel 9a physical-device capture)
- Compared state: Android, portrait, idle recording, system audio off, empty meeting list
- Normalization note: the source is a composite concept board rather than a device-native viewport. The mobile concept column was cropped and compared at equal rendered height; no claim of pixel-identical scaling is made.

## Full-view comparison

- Passed: one shared header contains the only `録音と会議` title.
- Passed: recording options, file selection, and meeting list form one flat screen without card containers.
- Passed: file selection is the first list item.
- Passed: idle state does not show elapsed time.
- Passed: the meeting-list fade communicates scrolling without covering the recording control.
- Passed: the mobile recording control remains a full-width-diameter semicircle attached to the bottom edge.

## Focused comparison

- Passed: the list-to-control transition follows the circle's curve instead of ending as a straight clipped line.
- Passed: the recording control's shadow remains visible above the circle and is not clipped by the list overlay.
- Passed: microphone and system-audio rows remain aligned without overlap.
- Passed: the common content surface keeps its intended rounded top corners.

## Iteration history

- Fixed P1: removed duplicate main-content title below the common header.
- Fixed P1: moved the bottom fade behind the recording control after it obscured the semicircle.
- Fixed P2: extended and softened the fade so the lower list edge no longer reads as a straight cut.
- Fixed P1: added a safe default for `onBusyChange` to prevent the recording screen from crashing during component initialization or hot reload.

## Required surfaces

- Default state: verified on the physical Android device.
- Empty state: verified; explanatory text appears without a redundant section heading.
- Responsive/mobile state: verified at the physical-device capture size.
- Loading/error state: no new visual treatment introduced by this change; existing shared handling remains in place.

## Result

Passed. No remaining P0, P1, or P2 visual mismatch was found in the requested mobile idle/empty state.
