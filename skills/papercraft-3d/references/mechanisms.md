# Pop-up mechanisms: the maths

Conventions: spine along x; floor `f = (0, 0, 1)` (toward the viewer); y up. Opening angle θ between the back page and
the floor (90° = an open 90° book, 180° = flat). Back-page direction `b(θ) = (0, sin θ, cos θ)`. Normal of anything
parallel to the back page: `n = x̂ × b = (0, −cos θ, sin θ)`.

## Box fold (parallel fold) on a 90° spread

Two parallel slits from `x0` to `x1`, running height `h` up the back page and depth `d` into the floor. Folds:
- valley at the back attach (height h)
- valley at the floor attach (depth d)
- mountain at the corner: on the flat strip it sits `d` below the back attach, which is at the spine when `h = d`

At any θ the strip is a parallelogram:

```
B = h·b          back attach
F = d·f          floor attach
C = B + F        corner (the mountain fold)
top face   = B → C   (length d, parallel to the floor)
front face = F → C   (length h, parallel to the back page)
```

- A card glued to the front face stands at `F + v·b + ε·n` (v up the card): parallel to the back page, at depth d.
- **Closed (θ → 0):** the corner reaches `h + d` from the spine, so `h + d ≤ page height`. A card of height p on
  the front face reaches `d + p`, so `d + p ≤ page`.
- Shallower card i vs. deeper box j (`d_j > d_i`): box j's top face crosses card i's plane at card height `h_j` for
  every θ. Notch card i over `[x0_j − c, x1_j + c]` above `h_j − c`.
- PopUpStory uses `h = d = a`. That's simplest to print (the mountain fold lies on the spine), and its closed limit is
  `2a ≤ page`.

```js
// three.js, per frame
const b = new V3(0, Math.sin(t), Math.cos(t)), f = new V3(0, 0, 1), x = new V3(1, 0, 0), up = new V3(0, 1, 0);
const n = new V3().crossVectors(x, b);
top.matrix.makeBasis(x, f.clone().negate(), up).setPosition(X(x0).addScaledVector(b, h).addScaledVector(f, d));
front.matrix.makeBasis(x, b, n).setPosition(X(x0).addScaledVector(f, d));
```

## Stacked boxes (a box on a box)

Cut the child across a **valley** whose opening angle equals θ. For example, use the parent's floor attach, where the
parent's front face (parallel to the back page) meets the floor in front of it. In that frame the front face is the
child's "back page" and the floor its "floor", so the same formulas apply with the origin at F: compose the
transforms, and the child is valid at every θ. Don't cut across the parent's corner C: that is a mountain fold, and a
box across it folds the wrong way.

## Floating layer (180° books)

A card parallel to the spine, held by two supports glued to the left and right pages at equal distance s from the
gutter, behaves like a parallel fold across the gutter. At 180° it stands vertical at height ≈ s. As the book closes
it tilts and lowers with the pages. Model each support as a parallelogram on its own page, exactly as above, in that
page's frame.

## V-fold (angled crease; 180° books)

A V-card's centre crease direction `v` keeps fixed angles to the two glue lines (paper is rigid):
`v·gL = cos αL`, `v·gR = cos αR`, `|v| = 1`. Here gL and gR are the glue-line directions on the left and right
pages, in world space for the current opening angle. Two cones intersect in at most two lines; take the upper one.

```js
// returns the upper unit vector v with v·a = ca, v·b = cb (a, b unit, not parallel)
function coneIntersect(a, b, ca, cb) {
  const ab = a.dot(b), det = 1 - ab * ab;
  const p = (ca - cb * ab) / det, q = (cb - ca * ab) / det;     // v = p·a + q·b + s·(a×b)
  const base = a.clone().multiplyScalar(p).addScaledVector(b, q);
  const s2 = (1 - base.lengthSq()) / det;                        // |a×b|² = det
  if (s2 < 0) return null;                                       // the paper would tear: angle unreachable
  const axb = new THREE.Vector3().crossVectors(a, b);
  const v = base.clone().addScaledVector(axb, Math.sqrt(s2));
  return v.y >= 0 ? v : base.addScaledVector(axb, -Math.sqrt(s2));
}
```

Each V-card panel then lies in the plane spanned by its glue line and `v`. `s2 < 0` means the design can't open to
that angle. Catch it in a design check, not at runtime.

## Pull tabs, wheels, sliders

These are mechanisms with their own input (a tab position, a wheel angle), not θ. Keep them as separate degrees of
freedom with their own springs, and check interference across the product of their ranges and θ.

## Print conventions (what PopUpStory's SVGs use)

| Line | Meaning | Style |
|---|---|---|
| solid red | cut | 0.3 mm |
| green dashed | valley fold (folds toward you) | 3 2 |
| orange dash-dot | mountain fold (folds away) | 4 1.5 1 1.5 |
| dotted box + label | glue area | 1 1 |

Print on 160–250 gsm card. Cut-outs carry ~0.8 mm bleed out to the cut line.
