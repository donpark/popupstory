// PopUpStory. Video → pop-up book, entirely in the browser: a <video> element decodes, the Rust core (WebAssembly) analyses and
// engineers the folds, and the papercraft player shows the result. Nothing is uploaded anywhere.
import init, { Session } from './pkg/popupstory.js';
import { BookPlayer } from './player.js';

const $ = (id) => document.getElementById(id);
const ANALYSIS_W = 320;
const ART_W = 1600;

const wasmReady = init();
let player = null;
let book = null;

// ---------------------------------------------------------------- video decoding
const once = (el, ev) => new Promise((res, rej) => {
  const ok = () => { el.removeEventListener('error', bad); res(); };
  const bad = () => { el.removeEventListener(ev, ok); rej(new Error('could not decode this video in the browser')); };
  el.addEventListener(ev, ok, { once: true });
  el.addEventListener('error', bad, { once: true });
});
async function seek(video, t) {
  if (Math.abs(video.currentTime - t) < 1e-3) return;
  video.currentTime = t;
  await once(video, 'seeked');
}
function grabber(video, w) {
  const h = Math.round((video.videoHeight * w) / video.videoWidth / 2) * 2;
  const c = document.createElement('canvas'); c.width = w; c.height = h;
  const g = c.getContext('2d', { willReadFrequently: true });
  return { w, h, canvas: c, grab: async (t) => { await seek(video, t); g.drawImage(video, 0, 0, w, h); return g.getImageData(0, 0, w, h).data; } };
}

// ---------------------------------------------------------------- optional depth model (the "minimal AI" switch)
let depthPipe = null;
async function modelDepth(canvas, status) {
  if (!depthPipe) {
    status('Loading the depth model (Depth Anything V2 small, ~25 MB, once)…');
    const { pipeline } = await import('https://cdn.jsdelivr.net/npm/@huggingface/transformers@3.5.1');
    const device = navigator.gpu ? 'webgpu' : 'wasm';
    depthPipe = await pipeline('depth-estimation', 'onnx-community/depth-anything-v2-small', { device });
  }
  const out = await depthPipe(canvas.toDataURL('image/jpeg', 0.9));
  const d = out.depth;   // RawImage, 1 channel, bright = near
  return { data: d.data, w: d.width, h: d.height };
}

// ---------------------------------------------------------------- pipeline
async function makeBook(file, { spreads, useModel }) {
  await wasmReady;
  const status = (msg, frac) => { $('status').textContent = msg; if (frac != null) $('bar').style.width = `${Math.round(frac * 100)}%`; };
  const url = URL.createObjectURL(file);
  const video = Object.assign(document.createElement('video'), { muted: true, playsInline: true, preload: 'auto', src: url });
  try {
    await once(video, 'loadedmetadata');
    const dur = video.duration;
    if (!isFinite(dur) || dur <= 0) throw new Error('this video has no readable duration');
    const fps = Session.analysis_fps(dur);
    const session = new Session(fps, spreads, 3, 5, 0.3);
    const lo = grabber(video, ANALYSIS_W);
    const n = Math.max(1, Math.floor(dur * fps));
    for (let i = 0; i < n; i++) {
      const t = Math.min((i + 0.5) / fps, dur - 0.05);
      session.push_frame(await lo.grab(t), lo.w, lo.h);
      status(`Reading the video… ${i + 1}/${n} frames`, 0.05 + 0.5 * (i + 1) / n);
    }
    const plan = JSON.parse(session.plan());
    const hi = grabber(video, Math.min(ART_W, video.videoWidth - (video.videoWidth % 2)));
    const keys = [];
    for (const [i, sp] of plan.spreads.entries()) {
      keys.push(await hi.grab(sp.time));
      if (useModel) {
        const d = await modelDepth(hi.canvas, status);
        session.set_depth(i, d.data, d.w, d.h);
      }
      status(`Choosing spreads… ${i + 1}/${plan.spreads.length}`, 0.55 + 0.15 * (i + 1) / plan.spreads.length);
    }
    status('Finding layers and engineering the folds…', 0.72);
    await new Promise((r) => setTimeout(r, 20));
    const still = JSON.parse(session.analyze());
    for (const [i, sp] of plan.spreads.entries()) {
      let samples = new Uint8Array(0), k = 0;
      if (still[i].camera_still) {
        const frames = [];
        for (const t of sp.samples) frames.push(await hi.grab(Math.min(t, dur - 0.05)));
        samples = new Uint8Array(frames.reduce((s, f) => s + f.length, 0));
        let off = 0; for (const f of frames) { samples.set(f, off); off += f.length; }
        k = frames.length;
      }
      status(`Printing spread ${i + 1}/${plan.spreads.length}…`, 0.75 + 0.25 * (i + 1) / plan.spreads.length);
      await new Promise((r) => setTimeout(r, 0));
      session.render_spread(i, keys[i], hi.w, hi.h, samples, k);
    }
    const title = file.name.replace(/\.[^.]+$/, '');
    const json = session.book_json(title);
    session.free();
    return JSON.parse(json);
  } finally {
    URL.revokeObjectURL(url);
  }
}

// ---------------------------------------------------------------- UI
function show(view) {
  for (const v of ['intro', 'working', 'viewer']) $(v).hidden = v !== view;
}

async function openBook(b) {
  book = b;
  show('viewer');
  $('book-title').textContent = b.title;
  $('print').disabled = !b.spreads.some((s) => s.sheets);
  if (!player) {
    player = new BookPlayer($('stage'), {
      onSpread: (i, s) => {
        $('count').textContent = `${i + 1} / ${book.spreads.length}`;
        $('meta').textContent = `from ${s.time} · ${s.pieces.length} pop-up layer${s.pieces.length === 1 ? '' : 's'} · depth: ${s.source}`;
        $('prev').disabled = i === 0;
        $('next').disabled = i === book.spreads.length - 1;
      },
    });
    window.__player = player;
  }
  await player.load(b);
}

async function handleFile(file) {
  if (!file) return;
  try {
    if (file.name.endsWith('.json')) return openBook(JSON.parse(await file.text()));
    show('working');
    const b = await makeBook(file, { spreads: +$('spreads').value, useModel: $('model').checked });
    await openBook(b);
  } catch (e) {
    console.error(e);
    show('intro');
    $('error').textContent = `Couldn't make a book: ${e.message}`;
  }
}

function printSheets() {
  const sheets = book.spreads.flatMap((s) => s.sheets || []);
  const html = `<!doctype html><meta charset="utf-8"><title>${book.title.replace(/</g, '&lt;')} - print sheets</title>
<style>@page{size:A4 portrait;margin:0}html,body{margin:0;background:#888}svg{display:block;width:210mm;height:297mm;margin:0 auto 8px;background:#fff;page-break-after:always}
@media print{body{background:none}svg{margin:0}}</style>${sheets.join('\n')}`;
  const w = window.open(URL.createObjectURL(new Blob([html], { type: 'text/html' })), '_blank');
  if (!w) alert('Allow pop-ups to open the print sheets.');
}

function download(name, text, type) {
  const a = Object.assign(document.createElement('a'), { href: URL.createObjectURL(new Blob([text], { type })), download: name });
  a.click(); setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

$('file').addEventListener('change', (e) => handleFile(e.target.files[0]));
$('sample').onclick = async () => {
  $('error').textContent = '';
  const blob = await (await fetch('./samples/big-buck-bunny-10s.mp4')).blob();
  handleFile(new File([blob], 'Big Buck Bunny', { type: 'video/mp4' }));
};
const drop = $('drop');
drop.addEventListener('dragover', (e) => { e.preventDefault(); drop.classList.add('over'); });
drop.addEventListener('dragleave', () => drop.classList.remove('over'));
drop.addEventListener('drop', (e) => { e.preventDefault(); drop.classList.remove('over'); handleFile(e.dataTransfer.files[0]); });
$('prev').onclick = () => player.go(player.idx - 1);
$('next').onclick = () => player.go(player.idx + 1);
$('toggle').onclick = () => { player.sound.unlock(); player.setOpen(player.theta < 45); };
$('angle').oninput = (e) => player.setAngle(+e.target.value);
$('sound').onclick = () => { player.sound.unlock(); const m = !player.sound.muted; player.sound.setMuted(m); $('sound').textContent = m ? 'Sound off' : 'Sound on'; };
$('print').onclick = printSheets;
$('save').onclick = () => download(`${book.title}.book.json`, JSON.stringify(book), 'application/json');
$('again').onclick = () => { show('intro'); $('file').value = ''; };
addEventListener('keydown', (e) => {
  if (!player || $('viewer').hidden) return;
  if (e.key === 'ArrowRight') player.go(player.idx + 1);
  if (e.key === 'ArrowLeft') player.go(player.idx - 1);
  if (e.key === ' ') { e.preventDefault(); $('toggle').click(); }
});
setInterval(() => {
  if (!player || $('viewer').hidden) return;
  if (player.manual == null) $('angle').value = player.theta;
  $('toggle').textContent = player.theta > 45 ? 'Close' : 'Open';
}, 60);

// ?book=path/to/book.json opens a book made by the CLI.
const q = new URLSearchParams(location.search).get('book');
if (q) fetch(q).then((r) => r.json()).then(openBook).catch((e) => { $('error').textContent = `Couldn't load ${q}: ${e.message}`; });
