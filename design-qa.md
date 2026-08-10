# Design QA

## Sources

- Reference: `C:/Users/taich.000/AppData/Local/Temp/codex-clipboard-7c1fd0c7-fef4-48eb-bc97-1eadff28c990.png`
- Desktop implementation: `E:/MutsunaJP/mutsuna-echo/.local-notes/summary-transcription-settings-desktop.png`
- Mobile implementation: `E:/MutsunaJP/mutsuna-echo/.local-notes/summary-settings-mobile.png`

## Test conditions

- Desktop: Codex in-app browser, default 1280 × 720 viewport, development preview state.
- Mobile: Codex in-app browser, 390 × 844 CSS viewport, 1x screenshot density, development preview state.
- Data: long ACP model names, installed and uninstalled cloud transcription providers, installed local STT and VAD models.

## Comparison

- Full view: local and cloud transcription models are grouped into one bordered manager card. Each model uses the same row structure: identity/status on the left and its relevant action on the right.
- Focused view: the reference Select allowed a long label to collide with the chevron. The implementation reserves the chevron/action space, applies `min-width: 0`, and truncates only the label with an ellipsis.
- The per-provider summary default Select is placed directly left of Delete, only for installed agents. At mobile width the action area uses a bounded two-column grid so Delete remains visible.

## Iterations

1. Combined local and cloud transcription settings under one `文字起こしモデル` card.
2. Converted cloud API-key settings from separate nested cards to rows matching the local model and AI-agent managers.
3. Fixed the mobile API-key form after visual QA exposed an input shrinking to nearly zero width.
4. Fixed the mobile summary-agent action row after visual QA exposed the Delete button outside the viewport.
5. Opened the installed-agent model Select, selected another model, and verified the trigger updated.

## Result

passed
