//! Classical (non-ML) video analysis: shot detection, keyframe choice, clean-plate
//! background, and a heuristic "depth proxy" built from cheap monocular cues.
use image::{GrayImage, Luma, RgbImage};

use std::ops::Range;

/// A dense float map (row-major), used for depth and cue maps.
#[derive(Clone)]
pub struct FMap {
    pub w: usize,
    pub h: usize,
    pub d: Vec<f32>,
}

impl FMap {
    pub fn new(w: usize, h: usize) -> Self {
        Self { w, h, d: vec![0.0; w * h] }
    }
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.d[y * self.w + x]
    }
    /// Approximate Gaussian blur: two passes of a separable box filter.
    pub fn blur(&self, r: usize) -> FMap {
        if r == 0 {
            return self.clone();
        }
        let mut o = self.clone();
        for _ in 0..2 {
            o = o.box_pass(r, true).box_pass(r, false);
        }
        o
    }
    fn box_pass(&self, r: usize, horiz: bool) -> FMap {
        let (len, lines) = if horiz { (self.w, self.h) } else { (self.h, self.w) };
        let idx = |line: usize, i: usize| if horiz { line * self.w + i } else { i * self.w + line };
        let mut o = FMap::new(self.w, self.h);
        let mut pre = vec![0f32; len + 1];
        for line in 0..lines {
            for i in 0..len {
                pre[i + 1] = pre[i] + self.d[idx(line, i)];
            }
            for i in 0..len {
                let a = i.saturating_sub(r);
                let b = (i + r + 1).min(len);
                o.d[idx(line, i)] = (pre[b] - pre[a]) / (b - a) as f32;
            }
        }
        o
    }
    pub fn percentile(&self, p: f32) -> f32 {
        let mut s = self.d.clone();
        s.sort_by(|a, b| a.total_cmp(b));
        s[((s.len() - 1) as f32 * p) as usize]
    }
    /// Stretch so the lo..hi percentiles map to 0..1.
    pub fn normalize(mut self, lo_p: f32, hi_p: f32) -> FMap {
        let lo = self.percentile(lo_p);
        let hi = self.percentile(hi_p);
        let span = (hi - lo).max(1e-6);
        for v in &mut self.d {
            *v = ((*v - lo) / span).clamp(0.0, 1.0);
        }
        self
    }
    pub fn to_gray(&self) -> GrayImage {
        GrayImage::from_fn(self.w as u32, self.h as u32, |x, y| {
            Luma([(self.at(x as usize, y as usize).clamp(0.0, 1.0) * 255.0) as u8])
        })
    }
}

// ---------------------------------------------------------------- shots

fn hist(img: &RgbImage) -> [f32; 64] {
    let mut h = [0f32; 64];
    for p in img.pixels() {
        let i = (p[0] as usize >> 6) * 16 + (p[1] as usize >> 6) * 4 + (p[2] as usize >> 6);
        h[i] += 1.0;
    }
    let n = (img.width() * img.height()) as f32;
    h.iter_mut().for_each(|v| *v /= n);
    h
}

fn chi2(a: &[f32; 64], b: &[f32; 64]) -> f32 {
    0.5 * a
        .iter()
        .zip(b)
        .map(|(x, y)| (x - y).powi(2) / (x + y + 1e-9))
        .sum::<f32>()
}

/// Split into shots wherever the colour histogram jumps.
pub fn detect_shots(frames: &[RgbImage], thr: f32, min_len: usize) -> Vec<Range<usize>> {
    let hs: Vec<_> = frames.iter().map(hist).collect();
    let mut cuts = vec![0usize];
    for i in 1..frames.len() {
        if chi2(&hs[i - 1], &hs[i]) > thr && i - cuts.last().unwrap() >= min_len {
            cuts.push(i);
        }
    }
    cuts.push(frames.len());
    let mut shots: Vec<Range<usize>> = cuts.windows(2).map(|w| w[0]..w[1]).collect();
    if shots.len() > 1 && shots.last().unwrap().len() < min_len {
        let l = shots.pop().unwrap();
        shots.last_mut().unwrap().end = l.end;
    }
    shots
}

/// Pick `n` shots for spreads: the longest ones, in story order. If there are too
/// few shots, split the longest ones.
pub fn choose_spreads(mut shots: Vec<Range<usize>>, n: usize, min_len: usize) -> Vec<Range<usize>> {
    while shots.len() < n {
        let (i, longest) = shots
            .iter()
            .enumerate()
            .max_by_key(|(_, r)| r.len())
            .map(|(i, r)| (i, r.clone()))
            .unwrap();
        if longest.len() < 2 * min_len {
            break;
        }
        let mid = longest.start + longest.len() / 2;
        shots[i] = longest.start..mid;
        shots.insert(i + 1, mid..longest.end);
    }
    let mut idx: Vec<usize> = (0..shots.len()).collect();
    idx.sort_by_key(|&i| std::cmp::Reverse(shots[i].len()));
    idx.truncate(n);
    idx.sort();
    idx.into_iter().map(|i| shots[i].clone()).collect()
}

fn luma(img: &RgbImage) -> FMap {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut m = FMap::new(w, h);
    for (i, p) in img.pixels().enumerate() {
        m.d[i] = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0;
    }
    m
}

fn laplacian_abs(l: &FMap) -> FMap {
    let mut o = FMap::new(l.w, l.h);
    for y in 1..l.h - 1 {
        for x in 1..l.w - 1 {
            let c = l.at(x, y);
            let v = 4.0 * c - l.at(x - 1, y) - l.at(x + 1, y) - l.at(x, y - 1) - l.at(x, y + 1);
            o.d[y * l.w + x] = v.abs();
        }
    }
    o
}

fn sharpness(img: &RgbImage) -> f32 {
    let lap = laplacian_abs(&luma(img));
    let n = lap.d.len() as f32;
    let mean = lap.d.iter().sum::<f32>() / n;
    lap.d.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n
}

/// Sharpest frame in the middle 60% of the shot (avoids transitions and motion blur).
pub fn pick_key(frames: &[RgbImage], r: Range<usize>) -> usize {
    let len = r.len();
    let a = r.start + len / 5;
    let b = (r.start + len * 4 / 5).max(a + 1).min(r.end);
    (a..b)
        .max_by(|&i, &j| sharpness(&frames[i]).total_cmp(&sharpness(&frames[j])))
        .unwrap_or(r.start)
}

pub fn sample_indices(r: Range<usize>, n: usize) -> Vec<usize> {
    let len = r.len();
    if len <= n {
        return r.collect();
    }
    (0..n).map(|k| r.start + k * (len - 1) / (n - 1)).collect()
}

/// Per-pixel temporal median: removes moving subjects, giving a "clean plate".
pub fn median_image(imgs: &[&RgbImage]) -> RgbImage {
    let (w, h) = imgs[0].dimensions();
    let mut out = RgbImage::new(w, h);
    let mut buf = Vec::with_capacity(imgs.len());
    for y in 0..h {
        for x in 0..w {
            let mut px = [0u8; 3];
            for (c, slot) in px.iter_mut().enumerate() {
                buf.clear();
                buf.extend(imgs.iter().filter(|im| im.dimensions() == (w, h)).map(|im| im.get_pixel(x, y)[c]));
                buf.sort_unstable();
                *slot = buf[buf.len() / 2];
            }
            out.put_pixel(x, y, image::Rgb(px));
        }
    }
    out
}

fn lab(p: &image::Rgb<u8>) -> [f32; 3] {
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let (r, g, b) = (lin(p[0]), lin(p[1]), lin(p[2]));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Keep the sharp keyframe, but where something moved (differs from the clean plate),
/// paint in the plate. Removes moving subjects from the backdrop without the blur a
/// full median picks up from small camera drift.
pub fn remove_movers(key: &RgbImage, plate: &RgbImage) -> RgbImage {
    let (w, h) = (key.width() as usize, key.height() as usize);
    let mut d = FMap::new(w, h);
    for (i, (a, b)) in key.pixels().zip(plate.pixels()).enumerate() {
        d.d[i] = (0..3).map(|c| (a[c] as f32 - b[c] as f32).abs()).sum::<f32>() / (3.0 * 255.0);
    }
    let d = d.blur((w / 150).max(1));
    RgbImage::from_fn(w as u32, h as u32, |x, y| {
        let m = ((d.at(x as usize, y as usize) - 0.05) / 0.08).clamp(0.0, 1.0);
        let (a, b) = (key.get_pixel(x, y), plate.get_pixel(x, y));
        image::Rgb([0, 1, 2].map(|c| (a[c] as f32 * (1.0 - m) + b[c] as f32 * m) as u8))
    })
}

/// "Figure" map: how much each pixel stands out from the ground behind it. The typical
/// depth of each image row approximates the ground plane at that distance, so the floor
/// itself scores ~0 while things standing on it (a mound, a person, a tree) score high.
/// Pop-up layers come from this; how far forward each one sits comes from `depth`.
pub fn figure_map(depth: &FMap) -> FMap {
    let (w, h) = (depth.w, depth.h);
    let mut med: Vec<f32> = (0..h)
        .map(|y| {
            let mut row: Vec<f32> = depth.d[y * w..(y + 1) * w].to_vec();
            row.sort_by(|a, b| a.total_cmp(b));
            row[w * 2 / 5]
        })
        .collect();
    let r = (h / 20).max(1);
    let pre: Vec<f32> = std::iter::once(0.0).chain(med.iter().scan(0.0, |s, v| { *s += v; Some(*s) })).collect();
    for (y, m) in med.iter_mut().enumerate() {
        let (a, b) = (y.saturating_sub(r), (y + r + 1).min(h));
        *m = (pre[b] - pre[a]) / (b - a) as f32;
    }
    let mut f = FMap::new(w, h);
    for y in 0..h {
        for x in 0..w {
            f.d[y * w + x] = (depth.at(x, y) - med[y]).max(0.0);
        }
    }
    f.blur((w / 160).max(1)).normalize(0.02, 0.995)
}

/// Load an external depth map (8- or 16-bit grayscale) as nearness in 0..1.
/// `far_is_bright` is for metric/distance maps (e.g. Depth Pro 2 metres) — they are
/// converted to inverse depth, which spreads layers the way the eye sees them.
pub fn load_external_depth(path: &std::path::Path, far_is_bright: bool) -> anyhow::Result<FMap> {
    let img = image::open(path)?.to_luma16();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut m = FMap::new(w, h);
    for (i, p) in img.pixels().enumerate() {
        let v = p[0] as f32 / 65535.0;
        m.d[i] = if far_is_bright { 1.0 / (v + 0.02) } else { v };
    }
    Ok(m.normalize(0.01, 0.99))
}

pub struct DepthInfo {
    pub motion_reliable: bool,
    pub source: String,
}

/// Heuristic nearness map in 0..1 (1 = near the viewer). Cues:
/// - motion vs. the clean plate (moving things are usually the subject)
/// - focus (subjects are sharp, backgrounds often defocused)
/// - colour saliency (frequency-tuned, Achanta et al. 2009)
/// - vertical position (lower in frame ≈ closer on a ground plane)
/// - a mild centre bias (framing)
/// An external depth map (e.g. from an offline monocular depth model) overrides these.
pub fn depth_proxy(key: &RgbImage, plate: &RgbImage, shot_len: usize, external: Option<&FMap>) -> (FMap, DepthInfo) {
    let (w, h) = (key.width() as usize, key.height() as usize);

    // Motion: difference of the keyframe from the clean plate.
    let mut mot = FMap::new(w, h);
    for (i, (a, b)) in key.pixels().zip(plate.pixels()).enumerate() {
        mot.d[i] = (0..3).map(|c| (a[c] as f32 - b[c] as f32).abs()).sum::<f32>() / (3.0 * 255.0);
    }
    let mot = mot.blur(1);
    let g = mot.percentile(0.5);
    let p99 = mot.percentile(0.99);
    // If most of the frame changes the camera is moving and the plate is meaningless.
    let motion_reliable = shot_len >= 3 && g < 0.03 && p99 > 0.1;
    let mut motn = mot.clone();
    motn.d.iter_mut().for_each(|v| *v = (*v / 0.12).min(1.0));

    let r_big = (w / 40).max(1);
    let sharp = laplacian_abs(&luma(key)).blur(r_big).normalize(0.02, 0.98);

    let mut labs: Vec<FMap> = (0..3).map(|_| FMap::new(w, h)).collect();
    for (i, p) in key.pixels().enumerate() {
        let l = lab(p);
        for c in 0..3 {
            labs[c].d[i] = l[c];
        }
    }
    let means: Vec<f32> = labs.iter().map(|m| m.d.iter().sum::<f32>() / m.d.len() as f32).collect();
    let labs: Vec<FMap> = labs.iter().map(|m| m.blur(2)).collect();
    let mut sal = FMap::new(w, h);
    for i in 0..w * h {
        sal.d[i] = (0..3).map(|c| (labs[c].d[i] - means[c]).powi(2)).sum::<f32>().sqrt();
    }
    let sal = sal.normalize(0.02, 0.98);

    let mut d = FMap::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let vert = y as f32 / h as f32;
            let dx = (x as f32 / w as f32 - 0.5) * 2.0;
            let center = 1.0 - dx * dx;
            d.d[i] = if motion_reliable {
                0.5 * motn.d[i] + 0.15 * sal.d[i] + 0.12 * sharp.d[i] + 0.15 * vert + 0.08 * center
            } else {
                0.38 * sal.d[i] + 0.25 * sharp.d[i] + 0.22 * vert + 0.15 * center
            };
        }
    }
    let mut source = if motion_reliable {
        "motion + saliency + focus + position".to_string()
    } else {
        "saliency + focus + position (camera moving / static shot)".to_string()
    };
    if let Some(ext) = external {
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let e = ext.at(x * ext.w / w, y * ext.h / h);
                d.d[i] = if motion_reliable { 0.7 * e + 0.3 * motn.d[i] } else { e };
            }
        }
        source = format!("external depth map{}", if motion_reliable { " + motion" } else { "" });
    }
    (d.blur((w / 80).max(1)).normalize(0.02, 0.99), DepthInfo { motion_reliable, source })
}
