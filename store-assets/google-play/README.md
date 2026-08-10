# Google Play visual assets

## Common

- `common/app-icon-512.png` — 512 × 512 PNG
- `common/feature-graphic-1024x500.png` — 1024 × 500 PNG

## Phone

Four portrait screenshots are provided in `phone/` at 1080 × 1920. This meets
Google Play's 9:16 requirement and the four-screenshot promotional threshold.

## Tablets

- `tablet-7/` — two landscape screenshots at 1920 × 1080
- `tablet-10/` — two landscape screenshots at 1920 × 1080

The tablet images intentionally share the same 16:9 compositions. Both dimensions
meet the corresponding Google Play upload limits.

## Notes

- All files are PNG and below their Google Play size limits.
- The app icon is copied from the repository's canonical Tauri icon.
- Screenshots are store-listing compositions based on implemented Mutsuna Echo
  functionality; replace them with device captures later if Google Play review
  requests exact in-app screenshots.
- A promotional video is not included because Google Play requires a public or
  unlisted YouTube URL rather than an uploaded image asset.
