//! PopUpStory core: video frames in, pop-up book out. No I/O here, so the same code
//! runs in the CLI (frames from ffmpeg) and in the browser via WebAssembly (frames
//! from a <video> element).
pub mod analysis;
pub mod book;
pub mod layers;
pub mod layout;
pub mod render;

#[cfg(target_arch = "wasm32")]
mod wasm;
