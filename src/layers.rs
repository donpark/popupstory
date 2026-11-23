//! Turn depth into cut-out objects. Each object is one connected region that stands out
//! from the ground (the figure map), kept whole at a single depth; a region is only split
//! where its depths form clearly separate groups (a squirrel in front of a rock), never
//! sliced into depth bands.
use crate::analysis::FMap;
use image::{GrayImage, Luma};
use imageproc::distance_transform::Norm;
use imageproc::morphology;
use imageproc::region_labelling::{connected_components, Connectivity};
use std::collections::HashMap;

pub struct Piece {
    /// Depth rank within the spread (0 = farthest).
    pub layer: usize,
    /// Bounding box in analysis-frame pixels.
    pub bx: u32,
    pub by: u32,
    pub bw: u32,
    pub bh: u32,
    /// bw×bh, 255 inside the piece.
    pub mask: GrayImage,
    pub area: u32,
    /// Median nearness (0 = back, 1 = front) under the piece, from the depth map.
    pub near: f32,
    /// Edge-accurate mask from a full-resolution depth map, if one was available.
    pub fine: Option<Fine>,
}

/// A high-resolution mask and its bounding box in analysis-frame pixels (fractional).
pub struct Fine {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub m: GrayImage,
}

/// Average of `v` over the pixels of `m`, smoothed over radius `r`, where there is support.
fn masked_blur(v: &FMap, m: &GrayImage, r: usize) -> (FMap, FMap) {
    let (w, h) = (v.w, v.h);
    let mut num = FMap::new(w, h);
    let mut den = FMap::new(w, h);
    for (x, y, p) in m.enumerate_pixels() {
        if p[0] != 0 {
            let i = y as usize * w + x as usize;
            num.d[i] = v.d[i];
            den.d[i] = 1.0;
        }
    }
    (num.blur(r), den.blur(r))
}

/// Snap a piece's mask to depth edges using a full-resolution depth map (Depth Pro 2 is sharp
/// at object edges). The rough mask comes from a small, blurred map, so it carries a halo of
/// whatever is behind the subject (grass around a head). Near the edge, each pixel joins the
/// object if its depth is closer to the object's depth nearby than to the surroundings'.
pub fn refine(p: &mut Piece, ext: &FMap, frame_w: u32, frame_h: u32) {
    let (kx, ky) = (ext.w as f32 / frame_w as f32, ext.h as f32 / frame_h as f32);
    let r = ((ext.w as f32) * 0.012).round().max(3.0) as i64;
    let x0 = ((p.bx as f32 * kx) as i64 - 2 * r).max(0);
    let y0 = ((p.by as f32 * ky) as i64 - 2 * r).max(0);
    let x1 = ((((p.bx + p.bw) as f32) * kx) as i64 + 2 * r).min(ext.w as i64);
    let y1 = ((((p.by + p.bh) as f32) * ky) as i64 + 2 * r).min(ext.h as i64);
    let (w, h) = ((x1 - x0).max(1) as u32, (y1 - y0).max(1) as u32);
    let coarse = GrayImage::from_fn(w, h, |x, y| {
        let fx = ((x0 + x as i64) as f32 / kx) as i64 - p.bx as i64;
        let fy = ((y0 + y as i64) as f32 / ky) as i64 - p.by as i64;
        let inside = fx >= 0 && fy >= 0 && fx < p.bw as i64 && fy < p.bh as i64 && p.mask.get_pixel(fx as u32, fy as u32)[0] != 0;
        Luma([if inside { 255 } else { 0 }])
    });
    let mut e = FMap::new(w as usize, h as usize);
    for y in 0..h as usize {
        for x in 0..w as usize {
            e.d[y * w as usize + x] = ext.at(x0 as usize + x, y0 as usize + y);
        }
    }
    let ru = r.min(255) as u8;
    let inner = morphology::erode(&coarse, Norm::L2, ru);
    let outer = morphology::dilate(&coarse, Norm::L2, ru);
    let wide = morphology::dilate(&coarse, Norm::L2, (2 * r).min(255) as u8);
    let ring = GrayImage::from_fn(w, h, |x, y| Luma([if wide.get_pixel(x, y)[0] != 0 && outer.get_pixel(x, y)[0] == 0 { 255 } else { 0 }]));
    let global = |m: &GrayImage| {
        let mut v: Vec<f32> = m.enumerate_pixels().filter(|(_, _, q)| q[0] != 0).map(|(x, y, _)| e.at(x as usize, y as usize)).collect();
        if v.is_empty() { None } else { Some(median(&mut v)) }
    };
    let core = if inner.pixels().any(|q| q[0] != 0) { inner.clone() } else { coarse.clone() };
    let (Some(obj_g), Some(bg_g)) = (global(&core), global(&ring)) else { return };
    let rr = (2 * r) as usize;
    let (on, od) = masked_blur(&e, &core, rr);
    let (bn, bd) = masked_blur(&e, &ring, rr);
    let mut m = GrayImage::from_fn(w, h, |x, y| {
        let i = y as usize * w as usize + x as usize;
        if core.get_pixel(x, y)[0] != 0 && inner.get_pixel(x, y)[0] != 0 {
            return Luma([255]);
        }
        if outer.get_pixel(x, y)[0] == 0 {
            return Luma([0]);
        }
        let obj = if od.d[i] > 0.02 { on.d[i] / od.d[i] } else { obj_g };
        let bg = if bd.d[i] > 0.02 { bn.d[i] / bd.d[i] } else { bg_g };
        let keep = if obj > bg + 0.02 { e.d[i] >= 0.5 * (obj + bg) } else { coarse.get_pixel(x, y)[0] != 0 };
        Luma([if keep { 255 } else { 0 }])
    });
    // Keep what is connected to the core, close pinholes, smooth the outline a little.
    let (labels, stats) = components(&m);
    let keep: std::collections::HashSet<u32> = stats
        .keys()
        .copied()
        .filter(|l| core.enumerate_pixels().any(|(x, y, q)| q[0] != 0 && labels.get_pixel(x, y)[0] == *l))
        .collect();
    for (x, y, q) in m.enumerate_pixels_mut() {
        if !keep.contains(&labels.get_pixel(x, y)[0]) {
            q[0] = 0;
        }
    }
    fill_holes(&mut m);
    let m = morphology::open(&m, Norm::L2, 1);
    let (_, st) = components(&m);
    let Some(&(_, mx0, my0, mx1, my1)) = st.values().max_by_key(|s| s.0) else { return };
    let crop = image::imageops::crop_imm(&m, mx0, my0, mx1 - mx0 + 1, my1 - my0 + 1).to_image();
    let fine = Fine {
        x: (x0 as f32 + mx0 as f32) / kx,
        y: (y0 as f32 + my0 as f32) / ky,
        w: (mx1 - mx0 + 1) as f32 / kx,
        h: (my1 - my0 + 1) as f32 / ky,
        m: crop,
    };
    // Re-derive the analysis-resolution box and mask so the layout uses the clean edges too.
    let (bx, by) = (fine.x.floor().max(0.0) as u32, fine.y.floor().max(0.0) as u32);
    let bw = ((fine.x + fine.w).ceil() as u32).min(frame_w).saturating_sub(bx).max(1);
    let bh = ((fine.y + fine.h).ceil() as u32).min(frame_h).saturating_sub(by).max(1);
    p.mask = GrayImage::from_fn(bw, bh, |x, y| Luma([fine.sample(bx as f32 + x as f32 + 0.5, by as f32 + y as f32 + 0.5)]));
    p.area = p.mask.pixels().filter(|q| q[0] != 0).count() as u32;
    (p.bx, p.by, p.bw, p.bh) = (bx, by, bw, bh);
    p.fine = Some(fine);
}

impl Fine {
    /// Mask value at an analysis-frame position (nearest sample of the fine mask).
    pub fn sample(&self, fx: f32, fy: f32) -> u8 {
        let u = ((fx - self.x) / self.w * self.m.width() as f32).floor();
        let v = ((fy - self.y) / self.h * self.m.height() as f32).floor();
        if u < 0.0 || v < 0.0 || u >= self.m.width() as f32 || v >= self.m.height() as f32 {
            0
        } else {
            self.m.get_pixel(u as u32, v as u32)[0]
        }
    }
}

/// How much of the frame its main subject fills: (bbox height, area) as fractions of the
/// frame, for the largest connected "figure" region. Close-ups score high on both, and
/// they make poor pop-ups: one subject fills the page, gets scaled down to fold flat, and
/// leaves a large hole in the backdrop. Wide shots with figures standing in a scene work.
pub fn subject_scale(figure: &FMap) -> (f32, f32) {
    let (w, h) = (figure.w as u32, figure.h as u32);
    let m = GrayImage::from_fn(w, h, |x, y| Luma([if figure.at(x as usize, y as usize) >= 0.45 { 255 } else { 0 }]));
    let m = morphology::open(&m, Norm::LInf, ((w as f32) / 120.0).ceil().max(1.0) as u8);
    let labels = connected_components(&m, Connectivity::Eight, Luma([0u8]));
    let mut stats: HashMap<u32, (u32, u32, u32)> = HashMap::new(); // area, miny, maxy
    for (_, y, p) in labels.enumerate_pixels() {
        if p[0] != 0 {
            let e = stats.entry(p[0]).or_insert((0, y, y));
            e.0 += 1;
            e.1 = e.1.min(y);
            e.2 = e.2.max(y);
        }
    }
    stats
        .values()
        .max_by_key(|s| s.0)
        .map(|&(a, y0, y1)| ((y1 - y0 + 1) as f32 / h as f32, a as f32 / (w * h) as f32))
        .unwrap_or((0.0, 0.0))
}

/// True when the shot looks like a close-up (see `subject_scale`).
pub fn is_closeup(figure: &FMap) -> bool {
    let (hf, af) = subject_scale(figure);
    hf > 0.78 || af > 0.3
}

fn fill_holes(m: &mut GrayImage) {
    let (w, h) = m.dimensions();
    let mut seen = vec![false; (w * h) as usize];
    let mut stack: Vec<(u32, u32)> = Vec::new();
    for x in 0..w {
        stack.push((x, 0));
        stack.push((x, h - 1));
    }
    for y in 0..h {
        stack.push((0, y));
        stack.push((w - 1, y));
    }
    while let Some((x, y)) = stack.pop() {
        let i = (y * w + x) as usize;
        if seen[i] || m.get_pixel(x, y)[0] != 0 {
            continue;
        }
        seen[i] = true;
        if x > 0 { stack.push((x - 1, y)); }
        if x + 1 < w { stack.push((x + 1, y)); }
        if y > 0 { stack.push((x, y - 1)); }
        if y + 1 < h { stack.push((x, y + 1)); }
    }
    for y in 0..h {
        for x in 0..w {
            if !seen[(y * w + x) as usize] {
                m.put_pixel(x, y, Luma([255]));
            }
        }
    }
}

/// Cut-out layers: nested masks of the figure map (what stands up off the ground); each
/// piece remembers how near it is, which later sets its fold depth.
/// Threshold on the figure map for "stands out from the ground".
const FIGURE_T: f32 = 0.35;
/// Two depth groups inside one region count as separate objects when the nearer group is at
/// least this much nearer (relative), and the smaller group is a real part of the region.
const SPLIT_GAP: f32 = 0.25;
const SPLIT_MIN_SHARE: f32 = 0.15;
/// Minimum median nearness (0..1, normalised over the frame) for a cut-out.
const MIN_NEAR: f32 = 0.2;

fn median(v: &mut [f32]) -> f32 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// Otsu's threshold on a set of depth values: the split that best separates two groups.
fn otsu(vals: &[f32]) -> f32 {
    let bins = 64usize;
    let mut hist = vec![0f32; bins];
    for &v in vals {
        hist[((v.clamp(0.0, 1.0)) * (bins - 1) as f32) as usize] += 1.0;
    }
    let total = vals.len() as f32;
    let sum: f32 = hist.iter().enumerate().map(|(i, c)| i as f32 * c).sum();
    let (mut wb, mut sb, mut best, mut thr) = (0f32, 0f32, -1f32, 0.5f32);
    for (i, &c) in hist.iter().enumerate() {
        wb += c;
        if wb == 0.0 || wb == total {
            continue;
        }
        sb += i as f32 * c;
        let (mb, mf) = (sb / wb, (sum - sb) / (total - wb));
        let between = wb * (total - wb) * (mb - mf).powi(2);
        if between > best {
            best = between;
            thr = (i as f32 + 0.5) / (bins - 1) as f32;
        }
    }
    thr
}

/// Connected components of a full-frame mask as (label image, per-label stats).
fn components(m: &GrayImage) -> (image::ImageBuffer<Luma<u32>, Vec<u32>>, HashMap<u32, (u32, u32, u32, u32, u32)>) {
    let labels = connected_components(m, Connectivity::Eight, Luma([0u8]));
    let mut stats: HashMap<u32, (u32, u32, u32, u32, u32)> = HashMap::new(); // area, x0, y0, x1, y1
    for (x, y, p) in labels.enumerate_pixels() {
        if p[0] != 0 {
            let e = stats.entry(p[0]).or_insert((0, x, y, x, y));
            e.0 += 1;
            e.1 = e.1.min(x);
            e.2 = e.2.min(y);
            e.3 = e.3.max(x);
            e.4 = e.4.max(y);
        }
    }
    (labels, stats)
}

/// Split one object region into depth groups if (and only if) they are clearly separate.
fn split_by_depth(region: &GrayImage, depth: &FMap, min_area: u32, r: u8, level: u32) -> Vec<GrayImage> {
    let vals: Vec<f32> = region
        .enumerate_pixels()
        .filter(|(_, _, v)| v[0] != 0)
        .map(|(x, y, _)| depth.at(x as usize, y as usize))
        .collect();
    if level >= 2 || vals.len() < (2 * min_area) as usize {
        return vec![region.clone()];
    }
    let t = otsu(&vals);
    let (mut far, mut near): (Vec<f32>, Vec<f32>) = vals.iter().partition(|&&v| v < t);
    if far.is_empty() || near.is_empty() {
        return vec![region.clone()];
    }
    let share = far.len().min(near.len()) as f32 / vals.len() as f32;
    let (mf, mn) = (median(&mut far), median(&mut near));
    if share < SPLIT_MIN_SHARE || (mn - mf) / mn.max(1e-3) < SPLIT_GAP {
        return vec![region.clone()]; // one object whose depth just varies smoothly
    }
    let mut parts = Vec::new();
    for side_near in [false, true] {
        let m = GrayImage::from_fn(region.width(), region.height(), |x, y| {
            let inside = region.get_pixel(x, y)[0] != 0 && (depth.at(x as usize, y as usize) >= t) == side_near;
            Luma([if inside { 255 } else { 0 }])
        });
        let m = morphology::open(&m, Norm::LInf, r);
        let (labels, stats) = components(&m);
        for (l, st) in stats {
            if st.0 < min_area {
                continue;
            }
            let part = GrayImage::from_fn(m.width(), m.height(), |x, y| Luma([if labels.get_pixel(x, y)[0] == l { 255 } else { 0 }]));
            parts.extend(split_by_depth(&part, depth, min_area, r, level + 1));
        }
    }
    if parts.is_empty() { vec![region.clone()] } else { parts }
}

/// Cut-out objects: whole regions that stand out from the ground, grouped by depth.
pub fn extract(depth: &FMap, figure: &FMap, min_area_frac: f32, max_pieces: usize) -> Vec<Piece> {
    let (w, h) = (depth.w as u32, depth.h as u32);
    let min_area = (min_area_frac * (w * h) as f32) as u32;
    let r = ((w as f32) / 120.0).ceil().max(1.0) as u8;

    let m = GrayImage::from_fn(w, h, |x, y| Luma([if figure.at(x as usize, y as usize) >= FIGURE_T { 255 } else { 0 }]));
    let mut m = morphology::open(&morphology::close(&m, Norm::LInf, r * 2), Norm::LInf, r);
    fill_holes(&mut m);
    // Rows that are nearly all "figure" are the ground plane; the floor page already shows it.
    for y in 0..h {
        let n = (0..w).filter(|&x| m.get_pixel(x, y)[0] != 0).count();
        if n as f32 > 0.85 * w as f32 {
            (0..w).for_each(|x| m.put_pixel(x, y, Luma([0])));
        }
    }

    let (labels, stats) = components(&m);
    let mut pieces: Vec<Piece> = Vec::new();
    for (l, (area, _, _, x1, _)) in stats {
        let _ = x1;
        if area < min_area {
            continue;
        }
        let region = GrayImage::from_fn(w, h, |x, y| Luma([if labels.get_pixel(x, y)[0] == l { 255 } else { 0 }]));
        for part in split_by_depth(&region, depth, min_area, r, 0) {
            let (_, st) = components(&part);
            let Some(&(area, x0, y0, x1, y1)) = st.values().max_by_key(|s| s.0) else { continue };
            let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
            // Skip: full-width bands (usually the ground, they would hog every fold position),
            // anything touching the top of the frame (tree crowns against the sky: too tall to
            // fold flat, and the backdrop shows them anyway).
            if area < min_area || bw as f32 > 0.75 * w as f32 || y0 == 0 {
                continue;
            }
            let mask = GrayImage::from_fn(bw, bh, |x, y| Luma([part.get_pixel(x0 + x, y0 + y)[0]]));
            let mut ds: Vec<f32> = mask
                .enumerate_pixels()
                .filter(|(_, _, v)| v[0] != 0)
                .map(|(x, y, _)| depth.at((x0 + x) as usize, (y0 + y) as usize))
                .collect();
            let near = median(&mut ds);
            // Far things that only stand out against the sky (distant trees) stay in the backdrop.
            if near < MIN_NEAR {
                continue;
            }
            pieces.push(Piece { layer: 0, bx: x0, by: y0, bw, bh, mask, area, near, fine: None });
        }
    }
    // Depth rank (0 = farthest), then priority: big, near, central things pick fold positions first.
    let mut order: Vec<usize> = (0..pieces.len()).collect();
    order.sort_by(|&a, &b| pieces[a].near.total_cmp(&pieces[b].near));
    for (rank, &i) in order.iter().enumerate() {
        pieces[i].layer = rank;
    }
    let prio = |p: &Piece| {
        let cx = (p.bx as f32 + p.bw as f32 / 2.0) / w as f32;
        p.area as f32 * (0.6 + p.near) * (1.2 - (cx - 0.5).abs())
    };
    pieces.sort_by(|a, b| prio(b).total_cmp(&prio(a)));
    pieces.truncate(max_pieces);
    pieces
}
