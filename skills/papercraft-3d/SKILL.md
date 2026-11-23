---
name: papercraft-3d
description: >-
  Engineering insights for 3D papercraft and pop-up worlds in the browser (three.js): fold kinematics that stay
  physically valid at every angle, flat-foldability and interference rules, turning photos or video frames into
  cut-out cards, streaming worlds of tiles that rise and fold, performance budgets, and game design in paper
  (roguelikes, dioramas, pop-up books). Use when building a pop-up mechanism, a papercraft game or a Papertrails-like
  world, a PopUpStory-based game such as a roguelike, or when paper scenes collide, pop, flicker or will not fold
  flat. Complements xikhar/papercraft (the LOOK: material, light, lens, prints); this skill is the MECHANICS and
  SYSTEMS.
license: MIT
metadata:
  version: "0.1.0"
---

# Papercraft 3D: mechanics, systems and insights

Lessons from building PopUpStory (video → pop-up book) on top of the papercraft kit, and from reading Papertrails
(an endless pop-up world by the same author as papercraft). For how paper should LOOK (matte stock, printed cut edge,
coloured soft shadows, tilt-shift lens, layered prints), use the `papercraft` skill (github.com/xikhar/papercraft);
this skill assumes it.

## 1. Paper is a linkage, so derive, don't animate

The single most useful idea: **a paper construction has one degree of freedom per fold system.** Drive the fold
angle θ (or a progress p in 0..1, overshooting past 1) and compute every card's pose from it. Never tween cards
independently: independent tweens pass through each other and break the illusion the instant the camera moves.

- **Parallel folds** (box folds, steps, floating layers): every face stays parallel to one of the two pages. Poses
  are linear in the page directions `b(θ)` and `f`, so they are exact and cheap: see `references/mechanisms.md`.
- **Angled folds** (V-folds, 180° spreads): the crease direction is the intersection of two cones (the paper keeps
  its crease angles). Solve it in closed form per frame; there's a snippet in `references/mechanisms.md`.
- **Nested flaps** (a flap on a flap): the child follows the parent's progress with a stiff spring
  (`p'' = k (p_parent − p) + c (p_parent' − p')`), as Papertrails does. It lags a hair, which reads as real paper.
- Place cards with an explicit basis, `matrix.makeBasis(X, Y, Z).setPosition(P)` with `Z = X × Y`. A left-handed
  basis mirrors the card and flips its lighting.

## 2. It must fold flat, so check it like a paper engineer

A world can look right open and be impossible closed. Check these before calling a mechanism done:

1. **Closed extent:** every piece's farthest point from the spine when closed stays inside the page. For a box fold
   of depth d and height h: `d + h ≤ page`, plus the piece above it.
2. **No sideways overlap** between slits or glue areas of different mechanisms (leave ~3 mm, and ~7 mm from page
   edges).
3. **Interference at every angle, not just open:** sample θ ∈ {5°, 15°, …, 90°} and run paperkit's
   `cardClips(root)`. Bugs hide mid-fold; always look at ~35°.
4. **Notch, don't overlap:** when a deeper box's top face must pass through a shallower standing card, cut a notch
   (≥ 1.5 mm clearance). With parallel folds the intersection sits at the same height on the card at every θ, so one
   notch is valid for the whole motion.
5. **After any cut, keep only paper connected to the glue or hinge.** Notches leave slivers that would fall out.
6. **Coplanar cards z-fight.** Real stacked cards have thickness: offset each by ≥ one card thickness along its
   normal (e.g. `1.2 × thickness + 0.04 × index`).

## 3. Photos and video frames → cards

- **Layers come from figure/ground, not raw depth.** Everything below the horizon is "near", so thresholding depth
  selects the floor. Subtract each image row's typical depth (a smoothed ~40th percentile ≈ the ground plane at that
  distance). What remains is what stands up. Use raw depth only to decide how far forward each piece sits.
- **One object, one piece, one depth.** Take each connected figure region whole at its median depth. Split it only
  where its depths form clearly separate groups. Slicing by depth bands (nested thresholds) cuts one character into
  two or three stacked copies. Score "stands out" on a fixed scale, not normalised per frame: one very near object
  would otherwise flatten every other figure. Drop full-width bands (the floor page already shows the ground) and
  anything far that only stands out against the sky.
- **Snap masks to depth edges.** Masks from a small, blurred map carry a halo of whatever is behind (grass round a
  head). With a sharp full-resolution depth map (Depth Pro 2 is very good at edges), assign the band around each
  mask by comparing a pixel's depth with the object's and the surroundings' local depth.
- **Put pieces back where they were.** Lift each cut-out on a box tall enough that its bottom sits at its height
  in the frame, and print the box face with the backdrop it hides. Head-on the page then reproduces the shot, and
  every other angle reveals the depth. Glued to the floor instead, pieces drop and shrink, and their filled-in
  ghosts show above them.
- **Fill what you cut out.** Remove each cut-out from the backdrop (a push-pull pyramid fill is enough, since it sits
  behind the piece) and remove nearer cut-outs from farther ones, or everything appears twice. Anything that can't
  fold flat at its real size (a trunk reaching the top of the page) stays in the backdrop rather than being shrunk.
- **Choose wide shots.** Close-ups make poor pop-ups: one subject fills the page and can't fold flat in place.
- **Bleed, never a white rim.** Print the real image ~0.8 mm past the subject, out to the cut line. A white border
  reads as a sticker (papercraft's look rules ban it too).
- **Flat base on the hinge line.** The glued edge is a fold, not a cut, so give it no printed rim (`folds:
  ['bottom']` in `makeCard`).
- **Simplify outlines** (Ramer–Douglas–Peucker, ~1 px at print resolution); morphological close then open before
  tracing. Spiky, thin silhouettes read as CG.
- **Don't paint movers out of the backdrop wholesale** with a temporal-median clean plate. It also erases subjects
  that never became cut-outs. Fill only what you actually cut out.
- **Cheap depth that runs locally:** Depth Anything V2 small via transformers.js on WebGPU runs about a second per
  frame after a one-time ~25 MB download. The heuristic fallback (motion, focus, saliency, vertical position) is
  fine for a still camera with one subject, and weak otherwise.

## 4. Rendering with paperkit: the practical bits

- `makeCard(outline, { paint })`: the print canvas has **y up in card units**. To draw an image:
  `g.save(); g.translate(0, h); g.scale(1, -1); g.drawImage(img, 0, 0, w, h); g.restore()`.
- `makeCard` `holes` must be interior. A cut that touches an edge (a strip cut out along the spine) belongs in the
  outline polygon as a notch.
- Big flat pages: `effects: 0` (print layers flat, much faster). Standing cut-outs keep effects for the cut edge.
- Units: the kit assumes 1 unit = 1 cm (card 0.035–0.08). Work in your own units in data and convert at the
  boundary.
- Soft shadows: `renderer.shadowMap.type = BasicShadowMap`, then `enableSoftShadows(key)` **before** the first
  render.
- `PaperPost`: set `post.focus` to the subject every frame; scale `aoRadius` and the haze distances to your scene
  size.
- WebGL textures from `file://` images are tainted, and ES modules and WASM won't load from `file://`. Serve over
  http, or embed images as data URIs.

## 5. Motion

- Drive openings with papercraft's `FlapSpring`: a fast rise, latch at upright, an 8–15 % overshoot with a quiver,
  settled by ~1 s, and a softer fall with one small slap. Bigger sheets get a larger `size` (slower).
- Page turns: close (spring to 0) → wait until settled → swap content → open. Build the next spread while the
  current one is open: printing a card costs ~10–30 ms.
- Ambient motion (wind) as a pure function of (time, position): deterministic, identical across frame rates.
- Fixed-step integration with an accumulator and interpolated rendering keeps springs deterministic for replays,
  captures and seeded runs.

## 6. Worlds and games in paper

For streaming worlds, procedural generation and a PopUpStory roguelike design, read `references/worlds-and-games.md`.
The short version:
- tiles rise inside a reveal radius ahead of the hero and fold flat behind
- a baked "printed twin" on the page means nothing ever pops
- generation is deterministic per chunk from a seed
- one draw call per material
- cards between the camera and the hero tip aside

## Checklist before you ship a paper scene

- [ ] Every pose derives from fold progress; nothing is tweened independently.
- [ ] Folds flat (closed extent), no overlaps, `cardClips` clean at several angles including ~35°.
- [ ] No coplanar z-fighting; no slivers after notches; glued edges have no rim.
- [ ] Cut-outs have bleed, not halos; outlines are simplified and chunky.
- [ ] Springs are fixed-step; ambient motion is a pure function; seeded randomness only.
- [ ] Prints are built off the critical path; instanced or batched where repeated.
