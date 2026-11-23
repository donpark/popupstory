//! CLI front-end: ffmpeg decodes, the core builds the book, files land on disk.
//! The same core runs in the browser (see web/).
mod video;

use anyhow::{bail, Result};
use clap::Parser;
use std::path::PathBuf;
use popupstory::{analysis, book};

#[derive(Parser)]
#[command(version, about = "Turn a video into a printable papercraft pop-up book")]
struct Args {
    /// Input video (anything ffmpeg can read)
    input: PathBuf,
    /// Output directory
    #[arg(short, long, default_value = "popup-book")]
    out: PathBuf,
    /// Number of spreads (pages) to make
    #[arg(long, default_value_t = 6)]
    spreads: usize,
    /// Depth layers per spread
    #[arg(long, default_value_t = 3)]
    layers: usize,
    /// Maximum cut-outs per spread
    #[arg(long, default_value_t = 5)]
    max_pieces: usize,
    /// Histogram distance that counts as a scene cut (0..1)
    #[arg(long, default_value_t = 0.3)]
    cut_threshold: f32,
    /// Folder of depth maps named spread_01.png… (8/16-bit, white = near unless
    /// --depth-far-is-bright). Overrides the heuristic cues.
    #[arg(long)]
    depth_dir: Option<PathBuf>,
    /// Command that writes a depth map for a keyframe, run once per spread;
    /// `{in}` / `{out}` = keyframe JPEG / output PNG (e.g. a Depth Pro 2 wrapper).
    #[arg(long)]
    depth_cmd: Option<String>,
    /// External depth maps encode distance (bright = far), e.g. metric depth.
    #[arg(long)]
    depth_far_is_bright: bool,
    /// Name of the depth model, shown in the book (e.g. "Depth Pro 2")
    #[arg(long)]
    depth_label: Option<String>,
    /// Keep close-up shots (by default they are skipped; wide shots make better pop-ups)
    #[arg(long)]
    keep_closeups: bool,
    /// Pixel width used for printed art
    #[arg(long, default_value_t = 1600)]
    art_width: u32,
    /// Embed the printable sheets in book.json (lets the web player offer "Print sheets")
    #[arg(long)]
    embed_sheets: bool,
    /// Book title (defaults to the file name)
    #[arg(long)]
    title: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if !args.input.exists() {
        bail!("input not found: {}", args.input.display());
    }
    let title = args.title.clone().unwrap_or_else(|| {
        args.input.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    });
    let out = &args.out;
    let cache = out.join(".cache");
    for d in ["sheets", "keyframes", "debug"] {
        std::fs::create_dir_all(out.join(d))?;
    }
    std::fs::create_dir_all(&cache)?;
    let tmp = cache.join("grab.png");

    let duration = video::probe_duration(&args.input)?;
    let fps = book::analysis_fps(duration);
    eprintln!("▸ decoding {duration:.1}s of video at {fps:.2} fps for analysis");
    let frames = video::extract_frames(&args.input, &cache.join("frames"), fps, 320)?;
    if frames.is_empty() {
        bail!("no frames decoded");
    }
    let opts = book::Options {
        spreads: args.spreads,
        layers: args.layers,
        max_pieces: args.max_pieces,
        cut_threshold: args.cut_threshold,
        skip_closeups: !args.keep_closeups,
    };
    let mut plan = book::plan_keys(&frames, fps, opts);
    eprintln!(
        "▸ {} frames, {} shots ({} close-ups skipped) → {} spreads",
        frames.len(), plan.shots, plan.closeups, plan.spreads.len()
    );
    let time_of = |plan: &book::Plan, i: usize| plan.time_of(i).min(duration - 0.05).max(0.0);

    // High-res keyframes first: they are the print art and the input to any depth model.
    let mut keys = Vec::new();
    for (i, sp) in plan.spreads.iter().enumerate() {
        let n = i + 1;
        let key = video::grab_frame(&args.input, time_of(&plan, sp.key), args.art_width, &tmp)?;
        let key_path = out.join(format!("keyframes/spread_{n:02}.jpg"));
        std::fs::write(&key_path, popupstory::render::encode_jpeg(&key))?;
        keys.push((key, key_path));
    }
    for (i, sp) in plan.spreads.iter_mut().enumerate() {
        let n = i + 1;
        let mut ext = args.depth_dir.as_ref().map(|d| d.join(format!("spread_{n:02}.png")));
        if let Some(cmd) = &args.depth_cmd {
            let dp = out.join(format!("debug/model_depth_{n:02}.png"));
            let line = cmd.replace("{in}", &keys[i].1.to_string_lossy()).replace("{out}", &dp.to_string_lossy());
            if !std::process::Command::new("sh").arg("-c").arg(&line).status()?.success() {
                bail!("depth command failed: {line}");
            }
            ext = Some(dp);
        }
        sp.external = ext
            .filter(|p| p.exists())
            .map(|p| analysis::load_external_depth(&p, args.depth_far_is_bright))
            .transpose()?;
    }
    book::analyze(&mut plan, &frames);
    if let Some(label) = &args.depth_label {
        for sp in plan.spreads.iter_mut() {
            if let Some(info) = sp.info.as_mut() {
                info.source = info.source.replace("external depth map", label);
            }
        }
    }

    let mut outs = Vec::new();
    for i in 0..plan.spreads.len() {
        let n = i + 1;
        let sp = &plan.spreads[i];
        sp.depth.as_ref().unwrap().to_gray().save(out.join(format!("debug/depth_{n:02}.png")))?;
        analysis::figure_map(sp.depth.as_ref().unwrap()).to_gray().save(out.join(format!("debug/figure_{n:02}.png")))?;
        let samples = if sp.info.as_ref().unwrap().motion_reliable {
            sp.hi_samples
                .iter()
                .map(|&f| video::grab_frame(&args.input, time_of(&plan, f), args.art_width, &tmp))
                .collect::<Result<Vec<_>>>()?
        } else {
            vec![]
        };
        let o = book::render_spread(&plan, i, &keys[i].0, &samples);
        std::fs::write(out.join(format!("sheets/spread_{n:02}_a_base.svg")), &o.base_svg)?;
        for (k, svg) in o.cutout_svgs.iter().enumerate() {
            std::fs::write(out.join(format!("sheets/spread_{n:02}_b_cutouts_{}.svg", k + 1)), svg)?;
        }
        eprintln!(
            "  spread {n}: t={:.1}s, {} cut-outs ({}){}",
            o.time,
            o.pieces.len(),
            o.source,
            if o.warnings.is_empty() { String::new() } else { format!(" — {}", o.warnings.join("; ")) }
        );
        outs.push(o);
    }
    std::fs::write(out.join("book.json"), book::book_json(&title, &outs, args.embed_sheets))?;
    std::fs::remove_dir_all(&cache).ok();
    eprintln!("✓ wrote {0}/book.json (open it in the web player) and printable sheets in {0}/sheets", out.display());
    Ok(())
}
