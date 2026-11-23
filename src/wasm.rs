//! WebAssembly session: the browser decodes frames from a <video> element and feeds
//! them in; everything else is the same core the CLI uses.
use crate::analysis::FMap;
use crate::book::{self, Options, Plan, SpreadOut};
use image::RgbImage;
use wasm_bindgen::prelude::*;

fn rgba_to_rgb(rgba: &[u8], w: u32, h: u32) -> RgbImage {
    let mut v = Vec::with_capacity((w * h * 3) as usize);
    for px in rgba.chunks_exact(4) {
        v.extend_from_slice(&px[..3]);
    }
    RgbImage::from_raw(w, h, v).expect("frame size mismatch")
}

#[wasm_bindgen]
pub struct Session {
    fps: f64,
    opts: Options,
    frames: Vec<RgbImage>,
    plan: Option<Plan>,
    outs: Vec<Option<SpreadOut>>,
}

#[wasm_bindgen]
impl Session {
    #[wasm_bindgen(constructor)]
    pub fn new(fps: f64, spreads: usize, layers: usize, max_pieces: usize, cut_threshold: f32) -> Session {
        Session {
            fps,
            opts: Options { spreads, layers, max_pieces, cut_threshold, skip_closeups: true },
            frames: vec![],
            plan: None,
            outs: vec![],
        }
    }

    /// Analysis frame rate for a video of this duration (seconds).
    pub fn analysis_fps(duration: f64) -> f64 {
        book::analysis_fps(duration)
    }

    /// Add one low-res analysis frame (RGBA from canvas getImageData).
    pub fn push_frame(&mut self, rgba: &[u8], w: u32, h: u32) {
        self.frames.push(rgba_to_rgb(rgba, w, h));
    }

    /// Phase 1. Returns JSON: {"shots":n,"spreads":[{"key":i,"time":t,"samples":[i…]}]}.
    pub fn plan(&mut self) -> String {
        let plan = book::plan_keys(&self.frames, self.fps, self.opts);
        let spreads: Vec<String> = plan
            .spreads
            .iter()
            .map(|s| {
                format!(
                    "{{\"key\":{},\"time\":{:.3},\"samples\":[{}]}}",
                    s.key,
                    plan.time_of(s.key),
                    s.hi_samples.iter().map(|i| format!("{:.3}", plan.time_of(*i))).collect::<Vec<_>>().join(",")
                )
            })
            .collect();
        let json = format!("{{\"shots\":{},\"closeups\":{},\"spreads\":[{}]}}", plan.shots, plan.closeups, spreads.join(","));
        self.outs = (0..plan.spreads.len()).map(|_| None).collect();
        self.plan = Some(plan);
        json
    }

    /// Optional: a depth map for spread `i` from a model (grayscale, white = near).
    pub fn set_depth(&mut self, i: usize, gray: &[u8], w: u32, h: u32) {
        if let Some(sp) = self.plan.as_mut().and_then(|p| p.spreads.get_mut(i)) {
            let mut m = FMap::new(w as usize, h as usize);
            for (k, v) in gray.iter().enumerate().take(m.d.len()) {
                m.d[k] = *v as f32 / 255.0;
            }
            sp.external = Some(m.normalize(0.01, 0.99));
        }
    }

    /// Phase 2: depth, layers, fold layout. Returns JSON: [{"camera_still":bool}…].
    pub fn analyze(&mut self) -> String {
        let plan = self.plan.as_mut().expect("plan() first");
        book::analyze(plan, &self.frames);
        let v: Vec<String> = plan
            .spreads
            .iter()
            .map(|s| format!("{{\"camera_still\":{}}}", s.info.as_ref().is_some_and(|i| i.motion_reliable)))
            .collect();
        format!("[{}]", v.join(","))
    }

    /// Phase 3 for spread `i`: high-res keyframe RGBA, plus `n_samples` sample frames
    /// concatenated in `samples` (same size; may be empty).
    pub fn render_spread(&mut self, i: usize, key: &[u8], w: u32, h: u32, samples: &[u8], n_samples: usize) {
        let plan = self.plan.as_ref().expect("plan() first");
        let key = rgba_to_rgb(key, w, h);
        let frame = (w * h * 4) as usize;
        let samples: Vec<RgbImage> =
            (0..n_samples).filter_map(|k| samples.get(k * frame..(k + 1) * frame)).map(|s| rgba_to_rgb(s, w, h)).collect();
        self.outs[i] = Some(book::render_spread(plan, i, &key, &samples));
    }

    /// The finished book as JSON (with printable SVG sheets embedded).
    pub fn book_json(&self, title: &str) -> String {
        book::book_json(title, self.outs.iter().flatten(), true)
    }

    /// Debug: the depth proxy for spread `i` as a grayscale PNG.
    pub fn depth_png(&self, i: usize) -> Vec<u8> {
        let g = self.plan.as_ref().and_then(|p| p.spreads.get(i)).and_then(|s| s.depth.as_ref()).map(|d| d.to_gray());
        let mut v = std::io::Cursor::new(Vec::new());
        if let Some(g) = g {
            let _ = g.write_to(&mut v, image::ImageFormat::Png);
        }
        v.into_inner()
    }
}
