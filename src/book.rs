//! The two-phase book pipeline.
//!
//! 1. `plan_keys`: low-res frames → spreads, keyframe per spread, frames to sample.
//! 2. `analyze`: depth (heuristic or external), cut-out layers, fold layout.
//! 3. `render_spread`: high-res keyframe (+ samples) → printable art, SVG sheets, player data.
//!
//! The split lets a caller fetch high-res frames (async in a browser) and run an
//! optional depth model on the keyframes between phases.
use crate::analysis::{self, DepthInfo, FMap};
use crate::layers::{self, Piece};
use crate::layout::{self, Crop, Placed, PAGE_H, PAGE_W};
use crate::render;
use image::RgbImage;
use std::fmt::Write as _;
use std::ops::Range;

#[derive(Clone, Copy)]
pub struct Options {
    pub spreads: usize,
    pub layers: usize,
    pub max_pieces: usize,
    pub cut_threshold: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self { spreads: 6, layers: 3, max_pieces: 5, cut_threshold: 0.3 }
    }
}

/// Analysis frame rate for a video of this length (≤ ~480 frames total).
pub fn analysis_fps(duration: f64) -> f64 {
    (480.0 / duration.max(0.1)).clamp(0.5, 6.0)
}

pub struct SpreadPlan {
    pub range: Range<usize>,
    pub key: usize,
    /// Frames to median for the clean plate at high resolution.
    pub hi_samples: Vec<usize>,
    pub external: Option<FMap>,
    pub depth: Option<FMap>,
    pub info: Option<DepthInfo>,
    pub pieces: Vec<Piece>,
    pub placed: Vec<Placed>,
    pub warnings: Vec<String>,
}

pub struct Plan {
    pub fps: f64,
    pub frame_w: u32,
    pub frame_h: u32,
    pub crop: Crop,
    pub opts: Options,
    pub shots: usize,
    pub spreads: Vec<SpreadPlan>,
}

impl Plan {
    pub fn time_of(&self, frame: usize) -> f64 {
        (frame as f64 + 0.5) / self.fps
    }
}

pub fn plan_keys(frames: &[RgbImage], fps: f64, opts: Options) -> Plan {
    let (fw, fh) = frames[0].dimensions();
    let min_len = ((fps * 1.0).round() as usize).max(3);
    let shots = analysis::detect_shots(frames, opts.cut_threshold, min_len);
    let n_shots = shots.len();
    let chosen = analysis::choose_spreads(shots, opts.spreads, min_len);
    let spreads = chosen
        .into_iter()
        .map(|r| SpreadPlan {
            key: analysis::pick_key(frames, r.clone()),
            hi_samples: analysis::sample_indices(r.clone(), 7),
            range: r,
            external: None,
            depth: None,
            info: None,
            pieces: vec![],
            placed: vec![],
            warnings: vec![],
        })
        .collect();
    Plan { fps, frame_w: fw, frame_h: fh, crop: layout::page_crop(fw, fh), opts, shots: n_shots, spreads }
}

pub fn analyze(plan: &mut Plan, frames: &[RgbImage]) {
    let thresholds = layers::thresholds(plan.opts.layers);
    for sp in plan.spreads.iter_mut() {
        let samples = analysis::sample_indices(sp.range.clone(), 15);
        let plate = analysis::median_image(&samples.iter().map(|&i| &frames[i]).collect::<Vec<_>>());
        let (depth, info) = analysis::depth_proxy(&frames[sp.key], &plate, sp.range.len(), sp.external.as_ref());
        let figure = analysis::figure_map(&depth);
        sp.pieces = layers::extract(&depth, &figure, &thresholds, 0.012, plan.opts.max_pieces);
        sp.warnings.clear();
        sp.placed = layout::layout(&sp.pieces, &plan.crop, plan.opts.layers, &mut sp.warnings);
        sp.depth = Some(depth);
        sp.info = Some(info);
    }
}

pub struct PieceOut {
    pub label: String,
    /// Left edge of the art on the page, mm from the left page edge.
    pub x: f32,
    /// Box-fold size = depth from the back page, mm.
    pub a: f32,
    pub w: f32,
    pub h: f32,
    pub png: Vec<u8>,
    /// Cut line in mm, art-local, y up, base on y = 0 (the glued bottom edge).
    pub outline: Vec<Vec<(f32, f32)>>,
}

pub struct SpreadOut {
    pub time: f64,
    pub source: String,
    pub backdrop_jpg: Vec<u8>,
    pub floor_jpg: Vec<u8>,
    pub floor: String,
    pub top: String,
    pub steps: Vec<(f32, f32, f32, String)>,
    pub pieces: Vec<PieceOut>,
    pub base_svg: String,
    pub cutout_svgs: Vec<String>,
    pub warnings: Vec<String>,
}

/// `key_hi`: the keyframe at print resolution. `samples_hi`: the plan's `hi_samples`
/// frames at the same resolution (may be empty; only used when the camera is still).
pub fn render_spread(plan: &Plan, i: usize, key_hi: &RgbImage, samples_hi: &[RgbImage]) -> SpreadOut {
    let sp = &plan.spreads[i];
    let n = i + 1;
    let info = sp.info.as_ref().expect("analyze() first");
    let (sx, sy) = (key_hi.width() as f32 / plan.frame_w as f32, key_hi.height() as f32 / plan.frame_h as f32);
    let usable: Vec<&RgbImage> = samples_hi.iter().filter(|f| f.dimensions() == key_hi.dimensions()).collect();
    let bg = if info.motion_reliable && usable.len() >= 3 {
        analysis::remove_movers(key_hi, &analysis::median_image(&usable))
    } else {
        key_hi.clone()
    };
    let bd = render::backdrop(&bg, &plan.crop, sx, sy);
    let backdrop_jpg = render::encode_jpeg(&bd);
    let floor_jpg = render::encode_jpeg(&render::floor_image(&bd));
    let floor = render::avg_bottom_color(&bd, 0.85);
    let top = render::avg_bottom_color(&bd, 1.05);
    let arts: Vec<render::Art> =
        sp.placed.iter().map(|pl| render::piece_art(&sp.pieces[pl.piece], pl, key_hi, sx, sy)).collect();
    let base_svg = render::base_svg(n, &backdrop_jpg, &floor_jpg, &top, &sp.placed);
    let items: Vec<_> = sp.placed.iter().zip(arts.iter()).collect();
    let cutout_svgs = render::pieces_svgs(n, &items);
    let pieces = sp
        .placed
        .iter()
        .zip(arts)
        .map(|(p, a)| PieceOut {
            label: p.label.clone(),
            x: p.x - a.pad_mm,
            a: p.a,
            w: a.w_mm,
            h: a.h_mm,
            outline: a.outline.iter().map(|poly| poly.iter().map(|&(x, y)| (x, a.h_mm - y)).collect()).collect(),
            png: a.png,
        })
        .collect();
    SpreadOut {
        time: plan.time_of(sp.key),
        source: info.source.clone(),
        backdrop_jpg,
        floor_jpg,
        floor,
        top,
        steps: sp.placed.iter().map(|p| (p.step.0, p.step.1, p.a, p.label.clone())).collect(),
        pieces,
        base_svg,
        cutout_svgs,
        warnings: sp.warnings.clone(),
    }
}

fn js_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// The book as JSON (images as data URIs). The web player reads this; `with_svgs`
/// includes the printable sheets so the browser can offer them for download.
pub fn book_json<'a>(title: &str, spreads: impl IntoIterator<Item = &'a SpreadOut>, with_svgs: bool) -> String {
    let b64 = render::b64;
    let mut j = String::new();
    let _ = write!(j, "{{\"title\":{},\"page\":{{\"w\":{PAGE_W},\"h\":{PAGE_H}}},\"spreads\":[", js_str(title));
    for (i, s) in spreads.into_iter().enumerate() {
        if i > 0 {
            j.push(',');
        }
        let t = s.time;
        let _ = write!(
            j,
            "{{\"time\":{},\"source\":{},\"backdrop\":\"data:image/jpeg;base64,{}\",\"floorImg\":\"data:image/jpeg;base64,{}\",\"floor\":\"{}\",\"top\":\"{}\",\"warnings\":[{}],\"steps\":[",
            js_str(&format!("{}:{:04.1}", (t / 60.0) as u32, t % 60.0)),
            js_str(&s.source),
            b64(&s.backdrop_jpg),
            b64(&s.floor_jpg),
            s.floor,
            s.top,
            s.warnings.iter().map(|w| js_str(w)).collect::<Vec<_>>().join(",")
        );
        for (k, (x0, x1, a, label)) in s.steps.iter().enumerate() {
            let _ = write!(j, "{}{{\"x0\":{x0:.2},\"x1\":{x1:.2},\"a\":{a:.2},\"label\":{}}}", if k > 0 { "," } else { "" }, js_str(label));
        }
        j.push_str("],\"pieces\":[");
        for (k, p) in s.pieces.iter().enumerate() {
            let outline: Vec<String> = p
                .outline
                .iter()
                .map(|poly| format!("[{}]", poly.iter().map(|(x, y)| format!("[{x:.2},{y:.2}]")).collect::<Vec<_>>().join(",")))
                .collect();
            let _ = write!(
                j,
                "{}{{\"label\":{},\"x\":{:.2},\"a\":{:.2},\"w\":{:.2},\"h\":{:.2},\"img\":\"data:image/png;base64,{}\",\"outline\":[{}]}}",
                if k > 0 { "," } else { "" },
                js_str(&p.label),
                p.x,
                p.a,
                p.w,
                p.h,
                b64(&p.png),
                outline.join(",")
            );
        }
        j.push(']');
        if with_svgs {
            let _ = write!(
                j,
                ",\"sheets\":[{}]",
                std::iter::once(&s.base_svg).chain(s.cutout_svgs.iter()).map(|x| js_str(x)).collect::<Vec<_>>().join(",")
            );
        }
        j.push('}');
    }
    j.push_str("]}");
    j
}
