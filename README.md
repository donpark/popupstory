# PopUpStory

Turn a short video into a pop-up book: a 3D papercraft book you can flip through in the browser, and printable A4
sheets to cut, fold and glue into the real thing.

**[Try it in your browser →](https://donpark.github.io/popupstory/)** Drop in a short video, or use the sample.

- **Web app** (`web/`): drop a video and it builds the book in your browser. Decoding uses a `<video>` element,
  analysis and paper engineering run in the Rust core compiled to WebAssembly, and rendering uses three.js with
  [papercraft](https://github.com/xikhar/papercraft)'s paper kit. Nothing is uploaded; a 10 s clip takes ~5 s.
- **CLI** (`src/main.rs`): the same core, with ffmpeg for decoding. It writes `book.json` plus the SVG sheets.

```
# web app
wasm-pack build --target web --release --no-default-features --out-dir web/pkg
python3 -m http.server -d web 8000          # open http://localhost:8000

# CLI (needs ffmpeg on PATH)
cargo run --release -- movie.mp4 -o my-book --spreads 6     # binary: popupstory
#   my-book/sheets/*.svg   print: one base sheet + cut-out sheet(s) per spread
#   my-book/book.json      drop it on the web app (or serve it and open ?book=path/to/book.json)
```

## How it works (no AI by default)

| Stage | Method |
|---|---|
| Spreads | colour-histogram shot detection → longest shots → sharpest frame in each |
| Backdrop | temporal median "clean plate"; moving subjects painted out of the sharp keyframe |
| Depth | heuristic nearness: motion vs. plate, focus, colour saliency, vertical position, centre bias |
| Layers | figure/ground map (depth minus each row's typical depth, a rough ground plane), so cut-outs are things *standing on* the ground, not the ground itself; nested thresholds → morphology → components |
| Paper engineering | 90° box folds; each piece's fold depth comes from its depth; `fold + piece height ≤ page`; folds never overlap; a shallower piece gets a notch where a deeper box's top face passes through it |
| Print | A4 SVG: backdrop on the top half, mirrored ground on the floor half, cut / valley / mountain lines, glue marks; cut-outs printed with bleed out to the cut line |
| Player | papercraft's `makeCard` turns each cut line + image into a card with thickness and a printed cut edge, plus matte paper shading, PCSS shadows, a tilt-shift lens and the latched flap spring for opening and closing |

The player uses the same kinematics as the paper. At opening angle θ every box strip is a parallelogram, so each
face stays parallel to the back page or the floor, and a spread that folds flat on screen folds flat in card.

## Optional: a depth model (the "minimal AI" switch)

The heuristic depth is the weak link. Real depth makes the figure/ground split much better: tree and mound come
forward instead of foreground grass.

- **Web:** tick *Use a depth model*. It runs Depth Anything V2 small via transformers.js, on WebGPU when available.
  The ~25 MB download happens once, then about a second per spread on the device.
- **CLI:** any local model, once per keyframe:
  ```
  popupstory movie.mp4 --depth-cmd 'python run_depth.py {in} {out}' --depth-far-is-bright
  popupstory movie.mp4 --depth-dir depth/      # precomputed spread_01.png… (white = near)
  ```
  `--depth-far-is-bright` handles metric/distance output, e.g. Apple Depth Pro 2 in metres, converted to inverse
  depth. Check model licences: Depth Pro 2's weights are research / non-commercial.

## Layout

```
src/lib.rs        core (no I/O): analysis, layers, layout, render, book (plan → analyze → render_spread)
src/wasm.rs       wasm-bindgen Session for the browser
src/main.rs       CLI (+ src/video.rs ffmpeg wrapper)
web/              index.html, app.js (pipeline), player.js (papercraft book), vendor/papercraft (MIT, see COMMIT)
tests/            end-to-end check that generated books obey the folding rules
```

## Credits

Paper rendering uses [papercraft](https://github.com/xikhar/papercraft) by xikhar (MIT). Sample video: Big Buck Bunny,
© Blender Foundation, CC BY 3.0. See [THIRD_PARTY.md](THIRD_PARTY.md). PopUpStory itself is MIT licensed.

## Roadmap

- **Share:** an embeddable player and a vertical clip of the book unfolding (Shorts / Reels), for creators.
- **Gallery:** creators publish pop-up versions of their own videos (YouTube sign-in to prove ownership), with a
  link back to the original video.
- **Desktop app:** Tauri around the same web UI with the Rust core running natively. Fully offline, reads any format
  via ffmpeg, and supports larger depth models.
- **Step inside:** an immersive mode where a spread opens into a walkable paper scene, in the spirit of
  [Papertrails](https://xikhar.github.io/papertrails/).

## Known limits

- Moving-camera shots lose the motion cue and the clean plate; global motion compensation would recover both.
- Nested layers can still produce near-duplicate cut-outs of one object.
- Objects that float in the frame get glued to the floor.
- 90° box folds only. 180° spreads need V-folds or floating layers on supports.
- The cut-out sheets don't nest pieces tightly (shelf packing).
