//! Thin wrapper over the ffmpeg / ffprobe CLIs (keeps the Rust build free of native video deps).
use anyhow::{bail, Context, Result};
use image::RgbImage;
use std::path::Path;
use std::process::Command;

pub fn probe_duration(input: &Path) -> Result<f64> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(input)
        .output()
        .context("running ffprobe (is ffmpeg installed and on PATH?)")?;
    if !out.status.success() {
        bail!("ffprobe failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.trim()
        .parse::<f64>()
        .with_context(|| format!("parsing duration '{}'", s.trim()))
}

/// Decode the whole video at a low frame rate and small size for analysis.
pub fn extract_frames(input: &Path, dir: &Path, fps: f64, width: u32) -> Result<Vec<RgbImage>> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)?;
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(input)
        .args(["-vf", &format!("fps={fps:.4},scale={width}:-2")])
        .arg(dir.join("f%05d.png"))
        .status()
        .context("running ffmpeg")?;
    if !status.success() {
        bail!("ffmpeg frame extraction failed");
    }
    let mut paths: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    paths.sort();
    paths.iter().map(|p| Ok(image::open(p)?.to_rgb8())).collect()
}

/// Grab a single high-resolution frame at time `t` seconds.
pub fn grab_frame(input: &Path, t: f64, width: u32, tmp: &Path) -> Result<RgbImage> {
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-ss", &format!("{t:.3}"), "-i"])
        .arg(input)
        .args(["-frames:v", "1", "-vf", &format!("scale={width}:-2")])
        .arg(tmp)
        .status()?;
    if !status.success() {
        bail!("ffmpeg failed grabbing frame at {t:.2}s");
    }
    Ok(image::open(tmp)?.to_rgb8())
}
