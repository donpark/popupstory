//! Paper engineering: place each cut-out on a 90° parallel ("box") fold.
//!
//! Boxes are not cubes: depth `d` sets how far forward a cut-out stands, height `h` lifts it
//! back to its height in the frame (the piece is glued at elevation `e` on the front face),
//! so seen head-on the spread reproduces the shot, like a tunnel book.
//!
//! One A4 sheet, folded across the middle, is one spread. The top half is the back
//! page, the bottom half the floor. A box fold is two parallel slits that run `h` up the
//! back page and `d` into the floor. Valley folds at both ends, and a mountain fold where
//! the top face meets the front face: `d` along the strip from the back attach (on the
//! back page if h > d, on the floor if h < d). With the sheet open at angle θ the strip is
//! a parallelogram: spine O, back attach B = h·b, floor attach F = d·f, corner C = B + F.
//! The top face (B→C) stays parallel to the floor and the front face (F→C) parallel to
//! the back page, so a cut-out glued to the front face stands at depth `d`.
//!
//! Constraints enforced here:
//!  * d + e + piece height ≤ page height, and d + h ≤ page height (folds flat when closed)
//!  * box folds never overlap sideways
//!  * a shallower piece that rises above a deeper box's top face gets a notch so the
//!    top face can pass through it (valid through the whole open/close motion,
//!    because every face stays parallel to either the back page or the floor).
use crate::layers::Piece;

pub const PAGE_W: f32 = 210.0; // mm, A4 width
pub const PAGE_H: f32 = 148.5; // mm, half of A4 height
const MIN_STEP: f32 = 10.0;
const GAP: f32 = 3.0;
const EDGE: f32 = 7.0;
const MIN_D: f32 = 8.0;
const NOTCH: f32 = 1.5;

/// Region of the analysis frame shown on the page ("cover" fit), in frame pixels.
#[derive(Clone, Copy)]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub fn page_crop(fw: u32, fh: u32) -> Crop {
    let (fw, fh) = (fw as f32, fh as f32);
    let aspect = PAGE_W / PAGE_H;
    if fw / fh > aspect {
        let w = fh * aspect;
        Crop { x: (fw - w) / 2.0, y: 0.0, w, h: fh }
    } else {
        let h = fw / aspect;
        Crop { x: 0.0, y: (fh - h) / 2.0, w: fw, h }
    }
}

pub struct Placed {
    pub piece: usize,
    pub label: String,
    /// Box depth: how far the front face (and the cut-out on it) stands from the back page, mm.
    pub d: f32,
    /// Box height: the top face sits this high above the floor, mm.
    pub h: f32,
    /// Elevation of the cut-out's bottom edge on the front face, mm. It lifts the piece back to
    /// where it was in the frame, so the spread reproduces the shot when seen head-on.
    pub e: f32,
    /// Left edge of the piece bbox and its size, mm.
    pub x: f32,
    pub wmm: f32,
    pub hmm: f32,
    /// Slit positions, mm from left page edge.
    pub step: (f32, f32),
    /// Notches as [x0, x1, z0] in piece-local mm (x from bbox left, z up from bottom).
    pub notches: Vec<[f32; 3]>,
}

fn find_slot(l: f32, r: f32, used: &[(f32, f32)]) -> Option<(f32, f32)> {
    let (l, r) = (l.max(EDGE), r.min(PAGE_W - EDGE));
    let span = r - l;
    if span < MIN_STEP {
        return None;
    }
    let c = (l + r) / 2.0;
    let mut width = (span * 0.7).max(MIN_STEP);
    while width >= MIN_STEP - 1e-3 {
        let mut off = 0.0f32;
        while off <= span / 2.0 {
            for sign in [1.0, -1.0] {
                let x0 = c - width / 2.0 + sign * off;
                let x1 = x0 + width;
                if x0 < l - 1e-3 || x1 > r + 1e-3 {
                    continue;
                }
                if used.iter().all(|&(u0, u1)| x1 + GAP <= u0 || x0 >= u1 + GAP) {
                    return Some((x0, x1));
                }
            }
            off += 2.0;
        }
        width *= 0.8;
    }
    None
}

pub fn layout(pieces: &[Piece], crop: &Crop, n_layers: usize, warnings: &mut Vec<String>) -> Vec<Placed> {
    let s0 = PAGE_W / crop.w;
    // Box depth follows how near each piece is, stretched over this spread's range.
    let (lo, hi) = pieces.iter().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p.near), h.max(p.near)));
    let span = (hi - lo).max(0.15);
    let level = |p: &Piece| PAGE_H * (0.08 + 0.30 * ((p.near - lo) / span).clamp(0.0, 1.0));
    let _ = n_layers;
    let mut used: Vec<(f32, f32)> = Vec::new();
    let mut placed: Vec<Placed> = Vec::new();

    // Pieces arrive in priority order; earlier ones get first pick of fold positions.
    for (i, p) in pieces.iter().enumerate() {
        // Keep each piece where it was in the frame: same size, same height on the page.
        let mut wmm = p.bw as f32 * s0;
        let mut hmm = p.bh as f32 * s0;
        let top = (p.by as f32 - crop.y) * s0;
        let mut e = (PAGE_H - top - hmm).max(0.0);
        let cx = (p.bx as f32 + p.bw as f32 / 2.0 - crop.x) * s0;
        // Fold flat: when closed the front face lies flat from d outward and the piece reaches
        // d + e + hmm from the spine. Prefer a shallower box, then lowering the piece a little,
        // then shrinking it a little. A piece that only fits much smaller (a trunk reaching
        // the top of the page) stays in the backdrop: shrunk or moved, it would no longer line
        // up with the picture and would show up twice.
        let room = PAGE_H - 1.0 - e - hmm; // space above the piece
        let mut d = level(p).max(MIN_D).min(room.max(MIN_D));
        let mut need = d + e + hmm - (PAGE_H - 1.0);
        if need > 0.0 {
            let lower = need.min(e).min(6.0);
            e -= lower;
            need -= lower;
        }
        if need > 0.0 {
            let f = (hmm - need) / hmm;
            if f < 0.85 {
                warnings.push(format!("kept a {wmm:.0}×{hmm:.0} mm piece in the backdrop: too tall to fold flat"));
                continue;
            }
            wmm *= f;
            hmm *= f;
        }
        d = d.min(PAGE_H - 1.0 - e - hmm).max(MIN_D.min(PAGE_H - 1.0 - e - hmm));
        let max_w = PAGE_W - 2.0 * EDGE;
        if wmm > max_w {
            let f = max_w / wmm;
            wmm *= f;
            hmm *= f;
        }
        let s = hmm / p.bh as f32; // mm per analysis pixel after any scaling
        let x = (cx - wmm / 2.0).clamp(EDGE / 2.0, PAGE_W - EDGE / 2.0 - wmm);
        // The piece is glued to the top band of the front face; the box rises to its top.
        let glue = (0.35 * hmm).clamp(8.0, 25.0).min(hmm);
        let h = e + glue;

        // Where is the piece solid in the band that will be glued to the front face?
        let band = ((glue / s).round() as u32).clamp(1, p.bh);
        let cols: Vec<u32> = (0..p.bw)
            .filter(|&c| {
                let n = (p.bh - band..p.bh).filter(|&y| p.mask.get_pixel(c, y)[0] != 0).count();
                n as f32 >= 0.3 * band as f32
            })
            .collect();
        let (l, r) = match (cols.first(), cols.last()) {
            (Some(&l), Some(&r)) => (x + l as f32 * s, x + (r + 1) as f32 * s),
            _ => (x, x + wmm),
        };
        // Prefer a fold hidden behind the solid base of the piece; otherwise anywhere under it.
        match find_slot(l, r, &used).or_else(|| find_slot(x, x + wmm, &used)) {
            Some(step) => {
                used.push(step);
                placed.push(Placed { piece: i, label: String::new(), d, h, e, x, wmm, hmm, step, notches: vec![] });
            }
            None => warnings.push(format!(
                "dropped a {:.0}×{:.0} mm layer-{} cut-out: no room for its fold",
                wmm, hmm, p.layer
            )),
        }
    }

    // Notches where a deeper box's top face (height h_j) passes through a shallower piece.
    let snapshot: Vec<(f32, f32, (f32, f32))> = placed.iter().map(|p| (p.d, p.h, p.step)).collect();
    for p in placed.iter_mut() {
        for &(dj, hj, (x0, x1)) in &snapshot {
            let (lo_z, hi_z) = (p.e, p.e + p.hmm);
            if dj <= p.d + 0.5 || hj + NOTCH <= lo_z || hj - NOTCH >= hi_z {
                continue;
            }
            let (n0, n1) = ((x0 - NOTCH).max(p.x), (x1 + NOTCH).min(p.x + p.wmm));
            if n1 > n0 {
                p.notches.push([n0 - p.x, n1 - p.x, (hj - NOTCH - p.e).max(0.0)]);
            }
        }
    }

    // Number back-to-front, left-to-right.
    placed.sort_by(|a, b| a.d.total_cmp(&b.d).then(a.x.total_cmp(&b.x)));
    for (n, p) in placed.iter_mut().enumerate() {
        p.label = format!("P{}", n + 1);
    }
    placed
}
