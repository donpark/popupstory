// Pop-up book player built on papercraft's paperkit-3d (MIT, github.com/xikhar/papercraft):
// real card thickness, printed cut edges lit on one side, dead-matte paper, PCSS soft shadows tinted by the world,
// a tilt-shift macro lens, and the latched paper-flap spring for opening and closing.
//
// The fold kinematics match the printed sheets exactly. At opening angle θ every box-fold strip is a parallelogram
// (spine O, back attach B = h·b, floor attach F = d·f, corner C = B + F), so every face stays parallel to either the
// back page or the floor. Each cut-out is glued at elevation e on its box's front face, which puts it back where it
// was in the frame: head-on the spread reproduces the shot, from any other angle it shows its depth.
import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import {
  paperTextures, paperMaterial, makeCard, paperLights, enableSoftShadows, PaperPost, FlapSpring,
} from './vendor/papercraft/paperkit-3d.js';
import { PaperSound } from './vendor/papercraft/paper-sound.js';

const CM = 0.1;          // book data is in mm; the paper kit's defaults assume 1 unit = 1 cm
const THICK = 0.035;     // ~350 gsm card
const loadImage = (src) => new Promise((res, rej) => { const i = new Image(); i.onload = () => res(i); i.onerror = rej; i.src = src; });

/** Paint part of an image (source rect in image pixels) into a w×h card print. */
const paintCrop = (img, sx, sy, sw, sh, w, h) => (P) => {
  const g = P.g;
  g.save(); g.translate(0, h); g.scale(1, -1); g.drawImage(img, sx, sy, sw, sh, 0, 0, w, h); g.restore();
};

/** Paint an image into a card print. Prints have y up in card units; images have y down. */
const paintImage = (img, w, h) => (P) => {
  const g = P.g;
  g.save(); g.translate(0, h); g.scale(1, -1); g.drawImage(img, 0, 0, w, h); g.restore();
};

export class BookPlayer {
  constructor(host, { onSpread } = {}) {
    this.onSpread = onSpread;
    const r = this.renderer = new THREE.WebGLRenderer({ antialias: false, preserveDrawingBuffer: true });
    r.setPixelRatio(Math.min(devicePixelRatio, 2));
    r.outputColorSpace = THREE.SRGBColorSpace;
    r.shadowMap.enabled = true;
    r.shadowMap.type = THREE.BasicShadowMap;   // the kit's PCSS replaces the filter
    host.prepend(r.domElement);

    this.scene = new THREE.Scene();
    this.scene.background = new THREE.Color('#5b4636');
    this.camera = new THREE.PerspectiveCamera(30, 1, 1, 400);
    this.camera.position.set(0, 30, 50);
    this.controls = new OrbitControls(this.camera, r.domElement);
    this.controls.target.set(0, 5, 4);
    this.controls.enableDamping = true;
    this.controls.maxPolarAngle = Math.PI * 0.47;
    this.controls.minDistance = 22; this.controls.maxDistance = 140;

    this.lights = paperLights(this.scene, { target: new THREE.Vector3(0, 4, 4), shadowExtent: 20, distance: 60, key: '#fff3e6', fill: '#f4ece2', sky: '#fffaf2' });
    enableSoftShadows(this.lights.key, { softness: 0.26, maxPenumbra: 1.0 });
    this.paper = paperTextures(r, { unitsPerTile: 6 });
    // Photos are printed on bright white, low-fibre stock: the warm default sheet multiplies
    // into every print and turns a film's palette orange.
    this.photoPaper = paperTextures(r, { color: '#ffffff', unitsPerTile: 6, fibres: 0.35, mottle: 0.015, normalStrength: 1.2, seed: 11 });

    // A desk and the book's boards (the brief is literally a book, so its edges stay in view).
    const desk = new THREE.Mesh(new THREE.PlaneGeometry(400, 400), paperMaterial({ color: '#a0826a', map: this.paper.map, normalMap: this.paper.normalMap }));
    desk.rotation.x = -Math.PI / 2; desk.position.y = -0.36; desk.receiveShadow = true;
    const board = paperMaterial({ color: '#7c3a2c', map: this.paper.map, normalMap: this.paper.normalMap });
    this.frontBoard = new THREE.Mesh(new THREE.BoxGeometry(1, 0.3, 1), board);
    this.backBoard = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 0.3), board);
    for (const m of [this.frontBoard, this.backBoard]) { m.castShadow = m.receiveShadow = true; }
    this.backBoard.matrixAutoUpdate = false;
    this.scene.add(desk, this.frontBoard, this.backBoard);

    this.post = new PaperPost(r, this.scene, this.camera, {
      maxBlur: 9, focusBand: 0.34, ramp: 0.4, aoRadius: 0.7, aoBias: 0.03, hazeStart: 80, hazeEnd: 220,
      // Printed photos already carry their own grade: keep the lens close to neutral.
      exposure: 0.9, tint: [1.0, 1.0, 1.0], saturation: 1.0, contrast: 1.04, greenDesat: 0.0, shadowLift: 0.02,
    });
    this.post.focus.set(0, 4, 5);

    this.spring = new FlapSpring({ size: 0.75 });
    this.sound = new PaperSound({ volume: 0.8 });
    this.sound.setMuted(true);
    this.cache = new Map();
    this.idx = -1;
    this.manual = null;   // fold angle set by the slider (overrides the spring)
    this.busy = false;

    this.resize = () => {
      const w = host.clientWidth, h = host.clientHeight;
      r.setSize(w, h); this.post.setSize(w, h);
      this.camera.aspect = w / h; this.camera.updateProjectionMatrix();
    };
    addEventListener('resize', this.resize);
    this.resize();
    let last = performance.now();
    r.setAnimationLoop((now) => {
      const dt = Math.min(0.05, Math.max(0, (now - last) / 1000)); last = now;
      this.spring.step(dt);
      this.update();
      this.controls.update();
      this.post.render(now / 1000);
    });
  }

  get theta() { return this.manual ?? 90 * this.spring.p; }

  /** base: URL the book was loaded from; asset paths in it are relative to that. */
  async load(book, base = location.href) {
    for (const s of this.cache.values()) this.scene.remove(s.group);
    this.cache.clear();
    this.book = book;
    this.base = base;
    const W = book.page.w * CM, H = book.page.h * CM;
    this.W = W; this.H = H;
    this.frontBoard.scale.set(W + 0.8, 1, H + 0.5);
    this.frontBoard.position.set(0, -0.2, H / 2 + 0.25);
    this.backBoard.scale.set(W + 0.8, H + 0.5, 1);
    this.idx = -1;
    this.spring.reset(0);
    await this.go(0);
  }

  async build(i) {
    if (this.cache.has(i)) return this.cache.get(i);
    const s = this.book.spreads[i], W = this.W, H = this.H, paper = this.photoPaper;
    const group = new THREE.Group();
    const steps = s.steps.map((st) => ({ x0: st.x0 * CM, x1: st.x1 * CM, h: st.h * CM, d: st.d * CM })).sort((p, q) => p.x0 - q.x0);
    const url = (src) => new URL(src, this.base).href;   // data: URLs pass through unchanged
    const [backImg, floorImg] = await Promise.all([loadImage(url(s.backdrop)), loadImage(url(s.floorImg))]);
    const card = (pts, opts) => {
      const m = makeCard(pts, { paper, thickness: THICK, edgeWidth: 0.05, pxPerUnit: 60, contact: { amount: 0.1, height: 0.8 }, ...opts });
      m.matrixAutoUpdate = false;
      // Photos printed on card: keep the fibre subtle and the stock's tone neutral.
      const face = Array.isArray(m.material) ? m.material[0] : m.material;
      face.normalScale?.set(0.22, 0.22);
      face.color?.set('#ffffff');
      group.add(m);
      return m;
    };

    // Back page: the box strips are cut out of it along the spine (they pop forward).
    const backPts = [[0, 0]];
    for (const st of steps) backPts.push([st.x0, 0], [st.x0, st.h], [st.x1, st.h], [st.x1, 0]);
    backPts.push([W, 0], [W, H], [0, H]);
    const back = card(backPts, { paint: paintImage(backImg, W, H), folds: ['bottom'], effects: 0 });
    // Floor: origin at the front-left corner, y toward the spine.
    const floorPts = [[0, 0], [W, 0], [W, H]];
    for (const st of [...steps].reverse()) floorPts.push([st.x1, H], [st.x1, H - st.d], [st.x0, H - st.d], [st.x0, H]);
    floorPts.push([0, H]);
    const floor = card(floorPts, { paint: paintImage(floorImg, W, H), folds: ['top'], effects: 0, contact: null });

    const faces = steps.map((st) => {
      const w = st.x1 - st.x0;
      // The front face carries the backdrop it hides, so head-on the box disappears.
      const k = backImg.naturalWidth / W;
      const front = paintCrop(backImg, st.x0 * k, (H - st.h) * k, w * k, st.h * k, w, st.h);
      return {
        st,
        top: card([[0, 0], [w, 0], [w, st.d], [0, st.d]], { paint: (P) => P.fill(s.top), folds: ['top', 'bottom'], contact: null }),
        front: card([[0, 0], [w, 0], [w, st.h], [0, st.h]], { paint: front, folds: ['top', 'bottom'], effects: 0 }),
      };
    });
    const pieces = await Promise.all(s.pieces.map(async (p, k) => {
      const img = await loadImage(url(p.img));
      const w = p.w * CM, h = p.h * CM;
      // The cut line from the core (mm, y up, base on y = 0). The kit wants one closed outline per card.
      const pts = p.outline.reduce((a, b) => (b.length > a.length ? b : a), []).map(([x, y]) => [x * CM, Math.max(0, y * CM)]);
      return { p, mesh: card(pts, { paint: paintImage(img, w, h), folds: ['bottom'], back: 'kraft' }), eps: THICK * 1.2 + 0.04 * k };
    }));
    const sp = { group, back, floor, faces, pieces, s };
    group.visible = false;
    this.scene.add(group);
    this.cache.set(i, sp);
    return sp;
  }

  /** Place every card for the current fold angle. */
  update() {
    const sp = this.cache.get(this.idx);
    if (!sp) return;
    const W = this.W, H = this.H;
    const t = THREE.MathUtils.degToRad(this.theta), sn = Math.sin(t), cs = Math.cos(t);
    const ex = new THREE.Vector3(1, 0, 0), b = new THREE.Vector3(0, sn, cs), f = new THREE.Vector3(0, 0, 1);
    const n = new THREE.Vector3().crossVectors(ex, b), up = new THREE.Vector3(0, 1, 0), negF = new THREE.Vector3(0, 0, -1);
    const X = (x) => new THREE.Vector3(x - W / 2, 0, 0);
    const place = (m, origin, Y, Z) => { m.matrix.makeBasis(ex, Y, Z).setPosition(origin); m.matrixWorldNeedsUpdate = true; };
    place(sp.back, X(0), b, n);
    place(sp.floor, new THREE.Vector3(-W / 2, 0, H), negF, up);
    for (const { st, top, front } of sp.faces) {
      const B = b.clone().multiplyScalar(st.h), F = f.clone().multiplyScalar(st.d);
      place(top, X(st.x0).add(B).add(F), negF, up);   // from the corner C back toward B, print side up
      place(front, X(st.x0).add(F), b, n);
    }
    for (const { p, mesh, eps } of sp.pieces) {
      place(mesh, X(p.x * CM).addScaledVector(f, p.d * CM).addScaledVector(b, p.e * CM).addScaledVector(n, eps), b, n);
    }
    this.backBoard.matrix.makeBasis(ex, b, n).setPosition(b.clone().multiplyScalar(H / 2 + 0.1).addScaledVector(n, -0.2 - THICK));
    this.backBoard.matrix.multiply(new THREE.Matrix4().makeScale(W + 0.8, H + 0.5, 1));
    this.backBoard.matrixWorldNeedsUpdate = true;
  }

  async waitSettled(target) {
    while (!(this.spring.atRest && Math.abs(this.spring.p - target) < 0.01)) await new Promise((r) => setTimeout(r, 30));
  }

  setOpen(open) {
    this.manual = null;
    this.spring.set(open ? 1 : 0);
    this.sound.flap({ size: 0.8, up: open });
  }

  setAngle(deg) { this.manual = deg; }

  async go(i) {
    if (this.busy || !this.book || i < 0 || i >= this.book.spreads.length) return;
    this.busy = true;
    try {
      const next = this.build(i);
      if (this.idx >= 0 && this.theta > 1) { this.setOpen(false); await this.waitSettled(0); }
      const sp = await next;
      for (const [k, s] of this.cache) s.group.visible = k === i;
      this.idx = i;
      this.update();
      this.onSpread?.(i, sp.s);
      this.setOpen(true);
      if (i + 1 < this.book.spreads.length) this.build(i + 1);   // pre-print the next spread
    } finally {
      this.busy = false;
    }
  }
}
