# UI redesign phase 4 — recent meetings

Phase 4 updates the recent-meetings area on the desktop home screen. Recording,
meeting selection, processing behavior, and compact/mobile composition retain
their existing behavior.

## Implemented

- Added a dedicated recent-meetings heading and moved audio import into a quiet
  secondary action beside it on desktop.
- Reworked desktop meeting rows into aligned title, date, size, and status
  columns with lightweight separators.
- Added exact audio duration beside each meeting's date and file size. Duration
  is cached for new audio at registration time. Older entries without a cached
  duration are inspected and saved only when their detail screen opens, so
  loading the meeting list never scans audio files. Entries without a cached
  duration simply omit it from the metadata line.
- Added labeled status treatments for recorded, imported, transcribed, missing
  audio, and active processing states. Status remains understandable without
  relying on color alone.
- Preserved source icons, meeting navigation, disabled states, keyboard focus,
  empty/loading messages, and the compact/mobile one-column meeting rows.
- Applied the recent-meetings heading and secondary audio-import action to the
  compact/mobile layout as well.
- Shared one heading, import action, meeting-row markup, status logic, and
  scroll container across desktop and mobile; responsive CSS only changes the
  column presentation and spacing.
- Added enough bottom scroll space on desktop and mobile for the final meeting
  to move completely above their respective fade regions.
- Replaced the inline input-settings expansion with one shared responsive
  popover. The home screen retains a compact source summary while microphone,
  system-audio, and silence-stop controls open on demand.
- Restored the complete `@mutsuna/ui` `AdminShellFrame` on desktop as well as
  mobile. Meetings, meeting detail, and settings now share the same shell
  header, sidebar trigger, inset content frame, and responsive behavior.

## Deliberately deferred

- Additional sidebar destinations or duplicate recent-meeting shortcuts
- Meeting detail and settings visual redesign
- Broader mobile visual redesign outside the recent-meetings area
- New persisted meeting metadata such as recording duration

## Verification

- Application-level visual and behavior verification is left to the user by
  request; no browser automation, build, or test command was run for this
  phase.
