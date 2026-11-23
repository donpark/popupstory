# Worlds and games in paper

Part A is how Papertrails (xikhar.github.io/papertrails, MIT) builds an endless pop-up world: techniques read from its
source, worth copying. Part B applies them, with PopUpStory, to a paper roguelike.

## A. Lessons from Papertrails

**Streaming that never pops**
- The world is tiles on an endless page. A tile **rises** when it is inside a reveal radius (3 tiles) around a point
  1.25 tiles *ahead* of the traveller, after a ripple delay that grows with distance. It **folds flat** beyond 4.25
  tiles and returns to a pool beyond 6. Rising ahead puts new tiles inside the top of the frame, where you watch them
  pop up.
- **Printed twin:** the page itself is one shader-drawn strip. A small data texture indexes an array of top-down bakes
  of every tile model, both its base print and the tile *with every flap folded flat*. A tile lying flat therefore
  matches the print under it exactly, so spawning, rising and despawning never pop. Un-risen cells show a faded
  print that "inks in" shortly before the tile rises.
- The book is one finite object (a spread, a page block, a cover) that follows the traveller with a dead zone. The
  print scrolls under its margins, and walking past a gutter line turns to the next spread.

**Deterministic generation**
- Wave function collapse on fixed 8×8 chunks, each generated from `(seed, chunk coords)` only. The world is
  identical in any visit order, and an evicted chunk regenerates identically.
- Seams are solved *before* collapse with stateless hash fields:
  - Rivers: each chunk corner gets a 0/1 label, and a border carries a river exactly when its two corners differ.
    Every chunk then has an even number of crossings, so rivers never dead-end.
  - Roads: each chunk owns one road to a hashed neighbour, so the network is connected.
- Inside a chunk, a planner routes the river and the trunk road first (noise-cost shortest paths). WFC then fills
  the rest:
  - lowest entropy first, with weighted picks
  - backtracking on contradiction, with bounded restarts
  - a guaranteed stub fill as the last resort
  - connector tiles that are never banned, so a solution always exists
- Simulation code never calls `Math.random` or the clock; everything comes from `hash2(seed, …)`.

**Performance**
- Every mesh of every tile is an instance in one `BatchedMesh` per material: one draw call per material, plus one in
  the shadow pass. The per-tile `Object3D` hierarchy exists only for the fold maths and is never added to the scene.
  Instance matrices are copied only while a tile is moving.
- No allocation per frame (even `Math.hypot` is avoided, since it allocates its rest-argument array). Pools for slots,
  in-place matrix writes.
- Collectables are one `InstancedMesh`: about 10 triangles per piece (card, strut, glue tab) and one icon atlas.

**Motion and feel**
- One degree of freedom per flap, integrated at a fixed sub-step, with four regimes:
  - RISE: a light spring that arrives fast
  - LOCKED: a stiff latch with air drag, giving the 8–15 % whip and a 4–7 Hz quiver
  - NESTED: follows its parent flap
  - FALL: a constant closing pull, then a < 3 % slap at the page

  Big flaps are slower and small ones snappier, plus a slight twist so cards aren't rigid boards.
- Wind is a pure function of (time, place): two travelling gust fronts, so neighbours sway one after another.

**Camera and readability**
- Long lens (fov ≈ 27°), ≈ 53° down, north up. A critically damped follow aims at a point *ahead* of the traveller,
  with the spring's lag pre-compensated, inside a still/soft/hard zone window.
- **Make room:** a small card (tree, bush, prop) standing in the sight line and tall enough to hide the traveller
  eases over to one side. If a large piece hides him for a moment, the camera tilts or yaws a few degrees until he's
  clear, then eases back.
- The map is a flat paper "atlas" page slid over the diorama. It costs nothing while closed and one draw call while
  open.

## B. A PopUpStory roguelike

The pitch: **each run is a book.** Floors are spreads, rooms rise as you enter, and the run ends when the book closes.
It looks like Papertrails and plays like a compact roguelike.

**World structure**
- A floor is a short Papertrails-style page (a chunk or two) rather than an endless one. Rooms are tile groups that
  rise together when the hero crosses their threshold: the classic roguelike room reveal, done as a pop-up.
- **Fog of war:** unexplored cells use the faded "printed twin". Explored but distant rooms fold flat again and stay
  inked. Line of sight decides what is risen.
- Doors, arches and caves are box folds with openings (papercraft law: what you walk into is built in 3D). Walkable
  height is open construction (decks, stairs on stringers) wired into the walk field.
- Use Papertrails-style WFC per floor with a guaranteed critical path (entrance → stairs), placed first as
  pre-decided sockets, like its roads.
- Seed everything: daily runs and shareable seeds come for free.

**Set-piece rooms from video (the PopUpStory bridge)**
- A `book.json` spread is a ready-made diorama: its backdrop becomes a room's back wall, and its cut-outs stand on
  box folds as scenery or enemies. Let players or creators drop in a clip to make a boss arena, a shrine or a story
  scene. Their YouTube video becomes a level, which is shareable content and a growth loop.
- Keep cut-outs from video as *scenery* or simple foes. Gameplay-critical actors need readable, purpose-built
  silhouettes.

**Actors and combat**
- Characters are small paper models (10–20 cards) with no cards cutting into each other at any pose (`cardClips` over
  the attack and walk cycles).
- Attacks are flap motions (slap, fold, swipe). Hits push a target's cards with an impulse into their springs (a
  quiver). Death = the actor folds flat onto the page with the soft-fall slap; loot pops up as a scrap card.
- Telegraph enemy intent with printed marks that pop up on a tab above the head. Silhouettes and accents must contrast
  with the ground (papercraft palette rules).
- Turn-based or real-time both work. Never block input on the ~1 s settle: let the next action start while the
  quiver finishes.

**Run end and sharing**
- Death or victory closes the book (`FlapSpring` to 0). The run summary is a generated pop-up spread of the path taken
  (rooms visited, boss, loot), viewable in the PopUpStory player and printable as a real card.

**Budgets that keep it smooth**
- Print (`makeCard`) each *kind* of card once per seed variant (4–6 per kind), bake to an atlas, then instance or
  batch. Never print per instance at runtime (10–30 ms each).
- One `BatchedMesh` or `InstancedMesh` per material. Write matrices only for moving cards.
- Run generation on a worker or between frames, and keep the next floor's prints ready before the stairs.
- Use fixed-step simulation and pure-function ambience, so replays and seeds are exact.
