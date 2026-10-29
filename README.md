<div align="left">

```
    __  ___            _ ____
   /  |/  /___ _____ _(_) __ \_________ __      __
  / /|_/ / __ `/ __ `/ / / / / ___/ __ `/ | /| / /
 / /  / / /_/ / /_/ / / /_/ / /  / /_/ /| |/ |/ /
/_/  /_/\__,_/\__, /_/_____/_/   \__,_/ |__/|__/
             /____/
```

<h3>Image-to-mouse drawing for Windows whiteboards</h3>
<p><i>Turn an image into mouse strokes for a game canvas.</i></p>

![Rust](https://img.shields.io/badge/Rust-2024_edition-b7410e?style=flat-square&logo=rust)
![Platform](https://img.shields.io/badge/platform-Windows-1a1a2e?style=flat-square)

</div>

## Overview

MagiDraw loads an image, converts it into strokes, and draws those strokes with Windows mouse input. It moves the cursor, holds left click for each stroke, and releases it before crossing blank space. The GUI shows the planned mouse paths before drawing.

## Features

| Feature | Description |
|---------|-------------|
| Conversion modes | Photo edges, thinned line art, and dark-area scanlines |
| Local balance | Shared 0–100% local contrast control for uneven lighting |
| Local detail | Adaptive detail detection for Dark areas and Line art |
| Enclosed fill | Optional horizontal fill inside closed edge regions |
| Preview | Shows the paths that will be sent to the mouse |
| Canvas capture | F6 marks the top-left corner; F7 marks the bottom-right corner |
| Global controls | F8 starts drawing; Esc stops and releases left click |
| Speed control | Shared 0.5× to 20× slider, with separate base timings under Advanced timing |
| Single executable | One Windows GUI binary; no Rust installation needed to run it |

## Build

On Linux, install Rust, the `x86_64-pc-windows-gnu` target, and MinGW. Then run:

```bash
rustup target add x86_64-pc-windows-gnu
make windows
make checksums
```

The versioned executable and SHA-256 file are written to `dist/`:

```text
dist/
├── magidraw-0.2.0-windows-x86_64.exe
└── magidraw-0.2.0-checksums.txt
```

For a native Windows build, run `cargo build --release`. The executable is `target\release\magidraw.exe`. `make check` runs formatting and Clippy checks for the Windows target.

## Usage

1. Click **Open image** and select a PNG, JPEG, WebP, GIF, or BMP file.
2. Hover over the whiteboard's top-left corner and press F6. Hover over its bottom-right corner and press F7. You can edit the coordinates in the GUI.
3. Pick a mode and adjust its controls until the preview shows the paths you want. Use **Local balance** to improve contrast across the image. In Dark areas or Line art, **Local detail difference** picks up marks that are darker than their nearby background even when they sit above the dark cutoff. Set it to 0 to turn it off; lower nonzero values detect fainter marks.
4. Set **Speed**. At 1×, Dark areas uses a 2 ms base move delay; Edges and Line art use 12 ms. Higher speeds also shorten the button and pen-up pauses.
5. Focus the game, then press F8. Drawing starts after the configured countdown. Press Esc to stop.

| Mode | Use |
|------|-----|
| Edges | Trace contours from photos. Smooth and edge strength control detail. **Fill enclosed areas** adds scanlines inside closed regions. |
| Line art | Thin dark ink to centerline strokes. Dark cutoff selects solid ink, and Local detail difference detects lighter marks. Short strokes are retained. |
| Dark areas | Draw horizontal runs wherever the image is darker than the cutoff or sufficiently darker than its surroundings. Use point spacing and the game's brush size to control coverage. |

The image keeps its aspect ratio and is centered in the marked rectangle. Speed is applied when drawing starts. If a game misses button releases or skips parts of a line at a high setting, lower the speed or adjust **Advanced timing**.

## How it works

MagiDraw resizes the image to the marked canvas, capped at 2048 pixels on the longest side for analysis. Local balance combines contrast-limited local histogram equalization with local detail recovery before any mode runs. It preserves the mean brightness of each tile, so solid dark regions stay dark. Set it to 0% for the original tones. Local detail uses `imageproc` adaptive thresholding to compare each pixel with its neighborhood. Edges mode uses bilateral smoothing and Canny detection from `imageproc`; Line art mode thins dark pixels until stable. Connected paths are simplified and joined at shared endpoints. Enclosed fill rows are joined only when the connector remains inside the filled region.

The drawing worker uses Windows `SendInput` with absolute mouse coordinates, non-coalesced move events, and a high-resolution waitable timer when available. Some games use raw input or reject injected mouse events; those games may ignore the drawing. Running MagiDraw at a lower privilege level than the game can also block input injection.

## Make targets

| Target | Result |
|--------|--------|
| `make windows` | Versioned release executable in `dist/` |
| `make windows-debug` | Debug executable in `dist/` |
| `make checksums` | SHA-256 file for the release executable |
| `make all` | Release executable and checksum |
| `make check` | Formatting and Windows-target Clippy checks |
| `make clean` | Remove Cargo and `dist/` build output |
