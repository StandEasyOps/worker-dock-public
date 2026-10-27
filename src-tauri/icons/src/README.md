# App icon sources

A1 mark and A7 tile from the Stand Easy Ops brand kit.

- `stand-easy-ops-tile-teal.svg` — render at **1024px or larger** (`rsvg-convert -w 1024`), then `npx tauri icon icons/app-icon-source.png`. Do not upscale the 512 PNGs. Drop the iOS/Android folders `tauri icon` adds.
- `stand-easy-ops-mark-white.svg` — header and tray (white on teal / dark). Tray status dots are painted by `scripts/render-tray-icons.py`; the mark stays one colour.
