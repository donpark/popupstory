//! Paper engineering: place each cut-out on a 90° parallel ("box") fold.
//!
//! One A4 sheet, folded across the middle, is one spread. The top half is the back
//! page, the bottom half the floor. A box fold of size `a` is two parallel slits that
//! run `a` up the back page and `a` down the floor; with the sheet open at angle θ the
//! strip forms a parallelogram: top face parallel to the floor at height a·sinθ, front
//! face parallel to the back page at distance a from the spine. A cut-out glued to
//! that front face stands parallel to the back page at depth `a`.
//!
//! Constraints enforced here:
//!  * a + piece height ≤ page height  (folds flat inside the closed book)
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
    /// Box-fold size (= depth from back page = step height), mm.
    pub a: f32,
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
    // Fold depth follows how near each piece is, stretched over this spread's range so
    // the nearest piece sits forward and the farthest just off the back page.
    let (lo, hi) = pieces.iter().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p.near), h.max(p.near)));
    let span = (hi - lo).max(0.15);
    let level = |p: &Piece| PAGE_H * (0.14 + 0.32 * ((p.near - lo) / span).clamp(0.0, 1.0));
    let _ = n_layers;
    let mut used: Vec<(f32, f32)> = Vec::new();
    let mut placed: Vec<Placed> = Vec::new();

    // Pieces arrive in priority order; earlier ones get first pick of fold positions.
    for (i, p) in pieces.iter().enumerate() {
        let mut s = s0;
        let mut wmm = p.bw as f32 * s;
        let mut hmm = p.bh as f32 * s;
        let a = level(p).min(hmm * 0.85).max(10.0);
        let max_h = PAGE_H - a - 2.0;
        let max_w = PAGE_W - 2.0 * EDGE;
        let f = (max_h / hmm).min(max_w / wmm).min(1.0);
        let cx = (p.bx as f32 + p.bw as f32 / 2.0 - crop.x) * s0;
        s *= f;
        wmm *= f;
        hmm *= f;
        let x = (cx - wmm / 2.0).clamp(EDGE / 2.0, PAGE_W - EDGE / 2.0 - wmm);

        // Where is the piece solid in the band that will be glued to the front face?
        let band = ((a / s).round() as u32).clamp(1, p.bh);
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
        // Prefer a fold hidden behind the solid base of the piece; otherwise anywhere
        // under it (the box face then reads as a small pedestal).
        match find_slot(l, r, &used).or_else(|| find_slot(x, x + wmm, &used)) {
            Some(step) => {
                used.push(step);
                placed.push(Placed { piece: i, label: String::new(), a, x, wmm, hmm, step, notches: vec![] });
            }
            None => warnings.push(format!(
                "dropped a {:.0}×{:.0} mm layer-{} cut-out: no room for its fold",
                wmm, hmm, p.layer
            )),
        }
    }

    // Notches where a deeper box's top face passes through a shallower piece.
    let snapshot: Vec<(f32, (f32, f32))> = placed.iter().map(|p| (p.a, p.step)).collect();
    for p in placed.iter_mut() {
        for &(aj, (x0, x1)) in &snapshot {
            if aj <= p.a + 0.5 || p.hmm <= aj - 1.5 {
                continue;
            }
            let (n0, n1) = ((x0 - 1.5).max(p.x), (x1 + 1.5).min(p.x + p.wmm));
            if n1 > n0 {
                p.notches.push([n0 - p.x, n1 - p.x, aj - 1.5]);
            }
        }
    }

    // Number back-to-front, left-to-right.
    placed.sort_by(|a, b| a.a.total_cmp(&b.a).then(a.x.total_cmp(&b.x)));
    for (n, p) in placed.iter_mut().enumerate() {
        p.label = format!("P{}", n + 1);
    }
    placed
}
