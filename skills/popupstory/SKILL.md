---
name: popupstory
description: >-
  Work on PopUpStory, the tool that turns a video into a 3D papercraft pop-up book (a three.js player plus printable
  A4 cut-and-fold sheets). The core is Rust, shared by a CLI and a WebAssembly build. Use when generating books from
  videos, changing the analysis / layering / fold layout / print sheets / player, reading or writing book.json,
  adding a pop-up mechanism, plugging in a depth model, embedding the player, or building on PopUpStory (gallery,
  desktop app, games). Not for general papercraft rendering style: see the papercraft-3d skill and xikhar/papercraft.
license: MIT
metadata:
  version: "0.1.0"
---

# PopUpStory

Video in, pop-up book out. Each spread is one A4 sheet folded to 90°: the top half is the **back page** (backdrop),
the bottom half the **floor**. Cut-outs stand on **box folds** cut from the sheet itself. The same numbers drive
three outputs that must never disagree: the printable SVG, `book.json`, and the 3D player.

## Run it

```bash
cargo test --release                         # end-to-end foldability check (tests/pipeline.rs)
cargo run --release -- clip.mp4 -o out --spreads 6        # CLI, needs ffmpeg; writes out/book.json + out/sheets/*.svg
wasm-pack build --target web --release --no-default-features --out-dir web/pkg
python3 -m http.server -d web 8000           # web app; must be served over http (WASM + ES modules fail on file://)
```

Open `http://localhost:8000/?book=/path/book.json` to view a CLI book. Pushing to `main` deploys `web/` to GitHub
Pages (`.github/workflows/pages.yml`).

## Map

| File | Role |
|---|---|
| `src/analysis.rs` | `FMap` float maps, shot detection (64-bin colour histogram χ²), keyframe = sharpest in the middle 60 %, temporal-median clean plate, `remove_movers`, heuristic `depth_proxy`, `figure_map`, `load_external_depth` |
| `src/layers.rs` | `extract`: nested figure-map masks → morphology → hole fill → ground-row strip → components → `Piece { mask, bbox, near }` |
| `src/layout.rs` | paper engineering: page crop, fold depth per piece, fold slots, notches, labels |
| `src/render.rs` | high-res cut-out art (bleed + traced cut line), backdrop/floor images, base and cut-out SVGs |
| `src/book.rs` | the pipeline API (below) and `book_json` |
| `src/wasm.rs` | `Session` for the browser (same phases) |
| `src/main.rs`, `src/video.rs` | CLI and its ffmpeg wrapper; the only I/O in the crate |
| `web/app.js` | browser decode (`<video>` seek + canvas), the WASM session, optional depth model, UI |
| `web/player.js` | `BookPlayer`: papercraft cards, fold kinematics, the open/close spring |
| `web/vendor/papercraft/` | xikhar/papercraft paper kit, vendored (MIT; commit in `COMMIT`) |

**Rule: the core does no I/O.** Frames come in as `RgbImage` and bytes or strings go out. New features go into the
core so the CLI, the web app and any future desktop app get them together.

## The pipeline (three phases, so callers can fetch frames asynchronously)

1. `plan_keys(frames, fps, opts)`: low-res frames (320 px wide, `analysis_fps = clamp(480/duration, 0.5, 6)`) →
   spreads (the longest shots, split if there are too few), a keyframe per spread, and `hi_samples`.
2. Optional: set `SpreadPlan.external` (a depth map) per spread. Then `analyze(plan, frames)`: depth, figure map,
   pieces, layout.
3. `render_spread(plan, i, key_hi, samples_hi)`: print-resolution frames (1600 px) → `SpreadOut` (art, SVGs, player
   data). Samples are only used when the camera was still (`DepthInfo.motion_reliable`).

## Coordinates and units (the source of most bugs)

- **Core and book.json: millimetres.** Page `PAGE_W = 210`, `PAGE_H = 148.5` (half of A4).
- **Sheet (SVG):** spine at y = 148.5. A back-page height `z` maps to `148.5 − z`; a floor depth `y` maps to
  `148.5 + y`.
- **Fold slots:** `step = (x0, x1)` in mm from the page's left edge; `a` = box size = depth from the back page =
  step height.
- **Cut-out art:** `x` = the art's left edge on the page. `outline` is art-local with **y up and the base on y = 0**
  (the glued edge; the art has no padding below). `w`/`h` = the art's size in mm.
- **Player: centimetres** (`CM = 0.1`), because paperkit's defaults assume 1 unit = 1 cm. Spine on the x axis,
  floor toward +z, y up.

## book.json

```jsonc
{ "title": "…", "page": { "w": 210, "h": 148.5 },
  "spreads": [{
    "time": "0:03.4", "source": "motion + saliency + …",       // which depth cues were used
    "backdrop": "data:image/jpeg;base64,…", "floorImg": "data:image/jpeg;base64,…",
    "floor": "#rrggbb", "top": "#rrggbb",                       // box front / top face colours
    "warnings": ["dropped a 43×32 mm cut-out: no room for its fold"],
    "steps":  [{ "x0": 61.2, "x1": 88.0, "a": 24.5, "label": "P1" }],
    "pieces": [{ "label": "P1", "x": 40.1, "a": 24.5, "w": 70.3, "h": 61.0,
                 "img": "data:image/png;base64,…", "outline": [[[x, y], …]] }],
    "sheets": ["<svg…base…>", "<svg…cut-outs…>"]                // web builds only
  }] }
```

Bump the format deliberately: the player, the CLI and saved books all read it.

## Paper-engineering invariants (enforced in layout.rs, asserted in tests/pipeline.rs)

1. **Folds flat:** `a + piece height ≤ PAGE_H` (pieces are scaled down to fit), and `2a ≤ PAGE_H` (the box corner
   reaches 2a from the spine when closed).
2. **No overlapping folds:** slots are `≥ GAP` (3 mm) apart, `≥ EDGE` (7 mm) from the page sides, `≥ MIN_STEP`
   (10 mm) wide. Placement prefers the piece's solid base, then anywhere under it; otherwise the piece is dropped
   with a warning.
3. **Notches:** a shallower piece taller than a deeper box's height gets a notch (1.5 mm clearance) where that box's
   top face passes through it. Because every face stays parallel to the back page or the floor, the notch clears at
   every opening angle. After notching, keep only the paper connected to the glued base (no slivers).
4. **Fold depth from depth:** `a = H·(0.14 + 0.32·rel_near)`, with `rel_near` normalised over that spread's pieces,
   clamped to `[10, 0.85·h]`.
5. **Bleed, not halo:** cut-outs print the real frame 0.8 mm past the subject, out to the cut line. Never a white
   rim.

## Depth and layers: what actually matters

- **Heuristic depth** (default, no ML) combines motion vs. clean plate, focus, Achanta saliency, vertical position
  and centre bias. Motion counts only when the camera is still: `shot ≥ 3 frames`, `median diff < 0.03`,
  `p99 > 0.1`.
- **Figure map, not raw depth, picks the layers.** Subtracting each image row's typical depth (a smoothed 40th
  percentile, a rough ground plane) makes the floor score ~0 and things standing on it score high. Without it, real
  depth just picks foreground grass.
- **External depth:** the web checkbox runs Depth Anything V2 small (transformers.js 3.5.1, WebGPU, ~25 MB once).
  The CLI takes `--depth-cmd '… {in} {out}'` or `--depth-dir`, with `--depth-far-is-bright` for metric depth
  (Depth Pro 2 output is converted to inverse depth). Check model licences: Depth Pro 2 is research-only.

## Adding a pop-up mechanism (V-fold, floating layer, pull tab…)

Touch all four places, or the print and the player will disagree:
1. **Geometry and constraints** in `layout.rs` (a new variant alongside the box fold, with its closed-state limits).
2. **Print** in `render.rs::base_svg`: cut solid red, valley green dashed, mountain orange dash-dot, plus glue
   marks.
3. **Kinematics** in `player.js::update()`: place every card from θ alone, with a right-handed basis `Z = X × Y`.
4. **Test** in `tests/pipeline.rs`: assert it folds flat and collides with nothing.

The maths for common mechanisms is in the papercraft-3d skill (`references/mechanisms.md`).

## Verifying a change

- `cargo test --release`, then rebuild the WASM and load the sample (`web/samples/big-buck-bunny-10s.mp4`).
- Drive the browser headlessly: set `input#file.files` from a `DataTransfer` and dispatch `change`. The player is
  `window.__player` (`.go(i)`, `.setAngle(deg)`, `.theta`, `.book`, `.waitSettled(1)`).
- Check a mid-fold angle (≈35°) as well as fully open: kinematic bugs only show in between.
- Rasterise sheets with `rsvg-convert -w 700 sheet.svg -o sheet.png` and look at the cut lines.

## Known gotchas

- ffmpeg `drawbox` ignores `t` expressions; use `overlay=x='…t…'` for moving test shapes.
- `xstack`-ing colour frames with grey depth maps turns the whole grid grey (format negotiation), not a bug.
- `wasm-pack` writes a `*` `.gitignore` into `pkg/`; CI deletes it before uploading Pages.
- Seeking a `<video>` is the decode bottleneck (~480 seeks max for long videos); show progress.
- paperkit's `makeCard` holes must be interior. Cuts that touch an edge (box strips at the spine) go into the
  outline as notches.
- A stray pointer event can orbit the camera in automated sessions; reload before judging framing.
- The default (no-ML) mode is weak on cluttered or panning shots. The depth model fixes most of it.
