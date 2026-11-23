//! Output: high-res cut-out art, printable SVG sheets, and data for the 3D player.
use crate::layers::Piece;
use crate::layout::{Crop, Placed, PAGE_H, PAGE_W};
use base64::Engine;
use image::{GrayImage, ImageFormat, Luma, RgbaImage, RgbImage};
use imageproc::contours::{find_contours, BorderType};
use imageproc::distance_transform::Norm;
use imageproc::geometry::approximate_polygon_dp;
use imageproc::morphology;
use std::fmt::Write as _;
use std::io::Cursor;

const MARGIN_MM: f32 = 0.8; // bleed beyond the subject's edge, out to the cut line

pub struct Art {
    /// The art as JPEG: the frame itself, cropped. The cut outline defines the shape (the
    /// player cuts the card along it, the print sheet clips to it), so no alpha is needed.
    pub jpg: Vec<u8>,
    pub w_mm: f32,
    pub h_mm: f32,
    /// Offset of the piece bbox's left edge inside the art, mm.
    pub pad_mm: f32,
    pub outline: Vec<Vec<(f32, f32)>>,
}

pub fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn encode_png(img: &RgbaImage) -> Vec<u8> {
    let mut v = Cursor::new(Vec::new());
    img.write_to(&mut v, ImageFormat::Png).unwrap();
    v.into_inner()
}

pub fn encode_jpeg(img: &RgbImage) -> Vec<u8> {
    let mut v = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut v, 84).encode_image(img).unwrap();
    v
}

/// A piece's mask at print resolution, in the frame of its analysis bounding box:
/// (left, top) in key pixels and the mask. Uses the edge-accurate mask when there is one.
pub fn hi_mask(p: &Piece, sx: f32, sy: f32) -> (i64, i64, GrayImage) {
    let hx = (p.bx as f32 * sx) as i64;
    let hy = (p.by as f32 * sy) as i64;
    let hw = ((p.bw as f32 * sx).round() as u32).max(1);
    let hh = ((p.bh as f32 * sy).round() as u32).max(1);
    let m = match &p.fine {
        Some(f) => GrayImage::from_fn(hw, hh, |x, y| {
            Luma([f.sample(p.bx as f32 + (x as f32 + 0.5) / sx, p.by as f32 + (y as f32 + 0.5) / sy)])
        }),
        None => {
            let r = image::imageops::resize(&p.mask, hw, hh, image::imageops::FilterType::Triangle);
            GrayImage::from_fn(hw, hh, |x, y| Luma([if r.get_pixel(x, y)[0] >= 128 { 255 } else { 0 }]))
        }
    };
    (hx, hy, m)
}

/// Render one cut-out at high resolution: image + bleed + notches, flat bottom.
pub fn piece_art(hm: &(i64, i64, GrayImage), pl: &Placed, key_hi: &RgbImage) -> Art {
    let (hx, hy, mask_hi) = (hm.0, hm.1, &hm.2);
    let (hw, hh) = mask_hi.dimensions();
    let m = pl.wmm / hw as f32; // mm per art pixel
    let margin_px = ((MARGIN_MM / m).ceil() as u32).clamp(1, 120);
    let pad = margin_px + 2;
    let (cw, ch) = (hw + 2 * pad, hh + pad); // no padding below: bottom edge sits on the floor
    let mut mask = GrayImage::new(cw, ch);
    for (x, y, v) in mask_hi.enumerate_pixels() {
        if v[0] >= 128 {
            mask.put_pixel(x + pad, y + pad, Luma([255]));
        }
    }
    let mut cut = morphology::dilate(&mask, Norm::L2, margin_px as u8);
    for n in &pl.notches {
        let c0 = pad + (n[0] / m).floor().max(0.0) as u32;
        let c1 = (pad + (n[1] / m).ceil() as u32).min(cw);
        let r1 = ((pad + hh) as f32 - n[2] / m).max(0.0) as u32;
        for y in 0..r1.min(ch) {
            for x in c0..c1 {
                mask.put_pixel(x, y, Luma([0]));
                cut.put_pixel(x, y, Luma([0]));
            }
        }
    }
    // Keep only paper connected to the glued bottom edge (drops slivers between notches).
    let mut keep = vec![false; (cw * ch) as usize];
    let mut stack: Vec<(u32, u32)> = (0..cw).filter(|&x| cut.get_pixel(x, ch - 1)[0] != 0).map(|x| (x, ch - 1)).collect();
    while let Some((x, y)) = stack.pop() {
        let i = (y * cw + x) as usize;
        if keep[i] || cut.get_pixel(x, y)[0] == 0 {
            continue;
        }
        keep[i] = true;
        if x > 0 { stack.push((x - 1, y)); }
        if x + 1 < cw { stack.push((x + 1, y)); }
        if y > 0 { stack.push((x, y - 1)); }
        if y + 1 < ch { stack.push((x, y + 1)); }
    }
    for y in 0..ch {
        for x in 0..cw {
            if !keep[(y * cw + x) as usize] {
                cut.put_pixel(x, y, Luma([0]));
                mask.put_pixel(x, y, Luma([0]));
            }
        }
    }
    let (kw, kh) = key_hi.dimensions();
    // The whole art rectangle is the frame itself: inside the cut line that is the subject
    // plus bleed (a slightly off cut never shows a white rim); outside it is never seen.
    let rgb = RgbImage::from_fn(cw, ch, |x, y| {
        let fx = (hx + x as i64 - pad as i64).clamp(0, kw as i64 - 1) as u32;
        let fy = (hy + y as i64 - pad as i64).clamp(0, kh as i64 - 1) as u32;
        *key_hi.get_pixel(fx, fy)
    });
    let _ = &mask;
    // Trace the cut line (add an empty row so the flat bottom is closed).
    let mut traced = GrayImage::new(cw, ch + 1);
    image::imageops::replace(&mut traced, &cut, 0, 0);
    let outline = find_contours::<i32>(&traced)
        .into_iter()
        .filter(|c| c.border_type == BorderType::Outer && c.points.len() >= 8)
        .map(|c| {
            approximate_polygon_dp(&c.points, 1.0, true)
                .into_iter()
                .map(|pt| (pt.x as f32 * m, pt.y.min(ch as i32) as f32 * m))
                .collect()
        })
        .collect();
    Art { jpg: encode_jpeg(&rgb), w_mm: cw as f32 * m, h_mm: ch as f32 * m, pad_mm: pad as f32 * m, outline }
}

/// Crop the high-res background to the page aspect ratio.
pub fn backdrop(src: &RgbImage, crop: &Crop, sx: f32, sy: f32) -> RgbImage {
    let (w, h) = src.dimensions();
    let x = (crop.x * sx) as u32;
    let y = (crop.y * sy) as u32;
    let cw = ((crop.w * sx) as u32).min(w - x);
    let ch = ((crop.h * sy) as u32).min(h - y);
    let c = image::imageops::crop_imm(src, x, y, cw, ch).to_image();
    image::imageops::resize(&c, 1400, (1400.0 * PAGE_H / PAGE_W) as u32, image::imageops::FilterType::Lanczos3)
}

/// Floor texture: the bottom of the backdrop mirrored outward, so the ground appears to
/// continue onto the floor page.
pub fn floor_image(bd: &RgbImage) -> RgbImage {
    let (w, h) = bd.dimensions();
    // Mirror a thin band of the backdrop's bottom at the spine (so the seam is invisible),
    // then fade into a soft, blurred version of the same ground. Tiling the band repeats its
    // rocks and leaves in rows; stretching it smears it into streaks; a deep band mirrors
    // trunks into "reflections".
    let bh = (h * 3 / 25).max(1);
    let band = image::imageops::crop_imm(bd, 0, h - bh, w, bh).to_image();
    let flipped = image::imageops::flip_vertical(&band);
    let soft = image::imageops::blur(&image::imageops::resize(&band, w, h, image::imageops::FilterType::Triangle), (w as f32) / 60.0);
    let mut f = RgbImage::new(w, h);
    for y in 0..h {
        // 1 near the spine, easing to 0 over the next two band-heights
        let t = (1.0 - (y as f32 - bh as f32 * 0.6) / (bh as f32 * 2.0)).clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        for x in 0..w {
            let sharp = if y < bh { *flipped.get_pixel(x, y) } else { *band.get_pixel(x, (y % bh).min(bh - 1)) };
            let s = soft.get_pixel(x, y);
            f.put_pixel(x, y, image::Rgb([0, 1, 2].map(|k| (sharp[k] as f32 * t + s[k] as f32 * (1.0 - t)) as u8)));
        }
    }
    for p in f.pixels_mut() {
        p.0 = p.0.map(|c| (c as f32 * 0.86) as u8);
    }
    f
}

pub fn avg_bottom_color(img: &RgbImage, darken: f32) -> String {
    let (w, h) = img.dimensions();
    let mut acc = [0f64; 3];
    let mut n = 0f64;
    for y in h - h / 10..h {
        for x in 0..w {
            let p = img.get_pixel(x, y);
            for c in 0..3 {
                acc[c] += p[c] as f64;
            }
            n += 1.0;
        }
    }
    let c: Vec<u8> = acc.iter().map(|v| ((v / n) * darken as f64).clamp(0.0, 255.0) as u8).collect();
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn path_d(outline: &[Vec<(f32, f32)>], ox: f32, oy: f32) -> String {
    let mut d = String::new();
    for poly in outline {
        for (i, (x, y)) in poly.iter().enumerate() {
            let _ = write!(d, "{}{:.2},{:.2} ", if i == 0 { "M" } else { "L" }, x + ox, y + oy);
        }
        d.push_str("Z ");
    }
    d
}

const CUT: &str = r##"stroke="#e00000" stroke-width="0.3" fill="none""##;
const VALLEY: &str = r##"stroke="#008a3e" stroke-width="0.3" stroke-dasharray="3 2" fill="none""##;
const MOUNTAIN: &str = r##"stroke="#e07000" stroke-width="0.3" stroke-dasharray="4 1.5 1 1.5" fill="none""##;

/// The base sheet: A4 portrait, backdrop on the top half, floor on the bottom half.
/// For each box: slits, the valley folds at both attach lines, the mountain fold between the
/// top face and the front face, the top face in the ground colour, and the front face printed
/// with the backdrop that is behind it, so the box vanishes when seen head-on.
pub fn base_svg(spread: usize, backdrop_jpg: &[u8], floor_jpg: &[u8], top: &str, placed: &[Placed]) -> String {
    let sp = PAGE_H; // spine y
    let mut s = String::new();
    let _ = writeln!(s, r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="210mm" height="297mm" viewBox="0 0 210 297" font-family="Helvetica, Arial, sans-serif">"#);
    let _ = writeln!(s, r#"<defs><image id="bd" href="data:image/jpeg;base64,{}" x="0" y="0" width="{PAGE_W}" height="{sp}" preserveAspectRatio="none"/></defs>"#, b64(backdrop_jpg));
    let _ = writeln!(s, r##"<use href="#bd"/>"##);
    let _ = writeln!(s, r#"<image href="data:image/jpeg;base64,{}" x="0" y="{sp}" width="{PAGE_W}" height="{sp}" preserveAspectRatio="none"/>"#, b64(floor_jpg));
    let mut steps: Vec<(f32, f32)> = placed.iter().map(|p| p.step).collect();
    steps.sort_by(|a, b| a.0.total_cmp(&b.0));
    for p in placed {
        let (x0, x1, h, d) = (p.step.0, p.step.1, p.h, p.d);
        let w = x1 - x0;
        let (ya, yf) = (sp - h, sp + d); // back attach, floor attach (sheet y)
        let ym = ya + d; // mountain fold: d along the strip from the back attach
        // Top face (ya..ym): ground colour. Front face (ym..yf): the backdrop rows it hides.
        let _ = writeln!(s, r#"<rect x="{x0:.2}" y="{ya:.2}" width="{w:.2}" height="{d:.2}" fill="{top}"/>"#);
        let _ = writeln!(s, r##"<svg x="{x0:.2}" y="{ym:.2}" width="{w:.2}" height="{h:.2}" viewBox="{x0:.2} {:.2} {w:.2} {h:.2}" preserveAspectRatio="none"><use href="#bd"/></svg>"##, sp - h);
        // Glue area: the top of the front face, from the box top down to the piece's bottom edge.
        let g = h - p.e;
        let _ = writeln!(s, r##"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="none" stroke="#333" stroke-width="0.2" stroke-dasharray="1 1"/>"##, x0 + 1.0, ym + 0.8, w - 2.0, (g - 1.6).max(0.5));
        let _ = writeln!(s, r##"<text x="{:.2}" y="{:.2}" font-size="3" text-anchor="middle" fill="#222" stroke="#fff" stroke-width="0.6" paint-order="stroke">glue {}</text>"##, x0 + w / 2.0, ym + g / 2.0 + 1.0, p.label);
        let _ = writeln!(s, r#"<line x1="{x0:.2}" y1="{ya:.2}" x2="{x0:.2}" y2="{yf:.2}" {CUT}/><line x1="{x1:.2}" y1="{ya:.2}" x2="{x1:.2}" y2="{yf:.2}" {CUT}/>"#);
        let _ = writeln!(s, r#"<line x1="{x0:.2}" y1="{ya:.2}" x2="{x1:.2}" y2="{ya:.2}" {VALLEY}/><line x1="{x0:.2}" y1="{yf:.2}" x2="{x1:.2}" y2="{yf:.2}" {VALLEY}/>"#);
        let _ = writeln!(s, r#"<line x1="{x0:.2}" y1="{ym:.2}" x2="{x1:.2}" y2="{ym:.2}" {MOUNTAIN}/>"#);
    }
    // Main spine valley fold, interrupted by the boxes.
    let mut cursor = 0.0;
    for (x0, x1) in steps.iter().copied().chain(std::iter::once((PAGE_W, PAGE_W))) {
        if x0 > cursor {
            let _ = writeln!(s, r#"<line x1="{cursor:.2}" y1="{sp}" x2="{x0:.2}" y2="{sp}" {VALLEY}/>"#);
        }
        cursor = x1;
    }
    let _ = writeln!(s, r##"<text x="4" y="293" font-size="2.6" fill="#222">Spread {spread} · base sheet · print on 160–250 gsm card ·  <tspan fill="#e00000">── cut</tspan>  <tspan fill="#008a3e">- - valley fold</tspan>  <tspan fill="#e07000">-·- mountain fold</tspan> · fold in half, push each box forward, glue each cut-out's bottom edge on its box's dotted line</text>"##);
    s.push_str("</svg>\n");
    s
}

/// Cut-out sheets (shelf-packed onto A4 portrait). Returns one SVG per sheet.
pub fn pieces_svgs(spread: usize, items: &[(&Placed, &Art)]) -> Vec<String> {
    let (mx, my, gap, label_h) = (8.0f32, 10.0f32, 5.0f32, 5.0f32);
    let mut sheets: Vec<String> = Vec::new();
    let mut body = String::new();
    let (mut x, mut y, mut row_h) = (mx, my, 0.0f32);
    let flush = |body: &mut String, sheets: &mut Vec<String>| {
        let n = sheets.len() + 1;
        sheets.push(format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="210mm" height="297mm" viewBox="0 0 210 297" font-family="Helvetica, Arial, sans-serif">
<rect width="210" height="297" fill="#fff"/>
{body}<text x="8" y="292" font-size="2.6" fill="#222">Spread {spread} · cut-outs sheet {n} · cut on the red line · glue each piece's bottom edge flush with the floor onto the matching "glue" box face</text>
</svg>
"##
        ));
        body.clear();
    };
    for (pl, art) in items {
        if x + art.w_mm > 210.0 - mx {
            x = mx;
            y += row_h + gap + label_h;
            row_h = 0.0;
        }
        if y + art.h_mm + label_h > 285.0 && !body.is_empty() {
            flush(&mut body, &mut sheets);
            x = mx;
            y = my;
            row_h = 0.0;
        }
        let d = path_d(&art.outline, x, y);
        let id = format!("s{spread}c{}", pl.label); // unique across spreads on one print page
        let _ = writeln!(body, r#"<clipPath id="{id}"><path d="{d}"/></clipPath>"#);
        let _ = writeln!(body, r#"<image href="data:image/jpeg;base64,{}" x="{x:.2}" y="{y:.2}" width="{:.2}" height="{:.2}" preserveAspectRatio="none" clip-path="url(#{id})"/>"#, b64(&art.jpg), art.w_mm, art.h_mm);
        let _ = writeln!(body, r#"<path d="{d}" {CUT}/>"#);
        let _ = writeln!(body, r##"<text x="{x:.2}" y="{:.2}" font-size="3" fill="#222">S{spread}-{} → box {} (depth {:.0} mm)</text>"##, y + art.h_mm + 3.5, pl.label, pl.label, pl.d);
        x += art.w_mm + gap;
        row_h = row_h.max(art.h_mm);
    }
    if !body.is_empty() || sheets.is_empty() {
        flush(&mut body, &mut sheets);
    }
    sheets
}
