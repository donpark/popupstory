//! Turn the depth proxy into a few nested cut-out layers ("tunnel book" style).
use crate::analysis::FMap;
use image::{GrayImage, Luma};
use imageproc::distance_transform::Norm;
use imageproc::morphology;
use imageproc::region_labelling::{connected_components, Connectivity};
use std::collections::HashMap;

pub struct Piece {
    pub layer: usize,
    /// Bounding box in analysis-frame pixels.
    pub bx: u32,
    pub by: u32,
    pub bw: u32,
    pub bh: u32,
    /// bw×bh, 255 inside the piece.
    pub mask: GrayImage,
    pub area: u32,
    /// Mean nearness (0 = back, 1 = front) under the piece, from the depth map.
    pub near: f32,
}

/// Figure-map thresholds for `n` nested layers (layer k = everything standing out more than t_k).
pub fn thresholds(n: usize) -> Vec<f32> {
    (0..n).map(|k| 0.3 + 0.5 * (k as f32 + 0.5) / n as f32).collect()
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
pub fn extract(depth: &FMap, figure: &FMap, thresholds: &[f32], min_area_frac: f32, max_pieces: usize) -> Vec<Piece> {
    let (w, h) = (depth.w as u32, depth.h as u32);
    let min_area = (min_area_frac * (w * h) as f32) as u32;
    let r = ((w as f32) / 120.0).ceil().max(1.0) as u8;
    let mut all: Vec<Piece> = Vec::new();

    for (k, &t) in thresholds.iter().enumerate() {
        let m = GrayImage::from_fn(w, h, |x, y| {
            Luma([if figure.at(x as usize, y as usize) >= t { 255 } else { 0 }])
        });
        let mut m = morphology::open(&morphology::close(&m, Norm::LInf, r * 2), Norm::LInf, r);
        fill_holes(&mut m);
        // Rows that are nearly all "near" are the ground plane; the floor page already
        // shows it. Removing them separates objects that stand on it.
        for y in 0..h {
            let n = (0..w).filter(|&x| m.get_pixel(x, y)[0] != 0).count();
            if n as f32 > 0.85 * w as f32 {
                (0..w).for_each(|x| m.put_pixel(x, y, Luma([0])));
            }
        }
        let labels = connected_components(&m, Connectivity::Eight, Luma([0u8]));
        // label -> (area, minx, miny, maxx, maxy)
        let mut stats: HashMap<u32, (u32, u32, u32, u32, u32)> = HashMap::new();
        for (x, y, p) in labels.enumerate_pixels() {
            if p[0] == 0 {
                continue;
            }
            let e = stats.entry(p[0]).or_insert((0, x, y, x, y));
            e.0 += 1;
            e.1 = e.1.min(x);
            e.2 = e.2.min(y);
            e.3 = e.3.max(x);
            e.4 = e.4.max(y);
        }
        for (l, (area, x0, y0, x1, y1)) in stats {
            let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
            if area < min_area {
                continue;
            }
            // Something covering nearly the whole frame is just the background again.
            // Full-width bands (usually the ground) are already represented by the floor
            // and would hog all the fold positions.
            if bw as f32 > 0.75 * w as f32 {
                continue;
            }
            let mask = GrayImage::from_fn(bw, bh, |x, y| {
                Luma([if labels.get_pixel(x0 + x, y0 + y)[0] == l { 255 } else { 0 }])
            });
            let mut sum = 0.0;
            for (x, y, v) in mask.enumerate_pixels() {
                if v[0] != 0 {
                    sum += depth.at((x0 + x) as usize, (y0 + y) as usize);
                }
            }
            all.push(Piece { layer: k, bx: x0, by: y0, bw, bh, mask, area, near: sum / area as f32 });
        }
    }

    // If a nearer layer reproduces almost all of a farther piece, keep only the nearer one.
    let covered = |p: &Piece, q: &Piece| -> u32 {
        let mut n = 0;
        for y in 0..q.bh {
            for x in 0..q.bw {
                if q.mask.get_pixel(x, y)[0] == 0 {
                    continue;
                }
                let (fx, fy) = (q.bx + x, q.by + y);
                if fx >= p.bx && fy >= p.by && fx < p.bx + p.bw && fy < p.by + p.bh
                    && p.mask.get_pixel(fx - p.bx, fy - p.by)[0] != 0
                {
                    n += 1;
                }
            }
        }
        n
    };
    let keep: Vec<bool> = (0..all.len())
        .map(|i| {
            !(0..all.len()).any(|j| {
                all[j].layer > all[i].layer && covered(&all[i], &all[j]) as f32 >= 0.8 * all[i].area as f32
            })
        })
        .collect();
    let mut pieces: Vec<Piece> = all.into_iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| p).collect();
    // Priority: big, near, central things first (they get first pick of fold positions).
    let prio = |p: &Piece| {
        let cx = (p.bx as f32 + p.bw as f32 / 2.0) / w as f32;
        p.area as f32 * (1.0 + 0.5 * p.layer as f32) * (1.2 - (cx - 0.5).abs())
    };
    pieces.sort_by(|a, b| prio(b).total_cmp(&prio(a)));
    pieces.truncate(max_pieces);
    pieces
}
