//! End-to-end: synthetic video → book, then check the paper-engineering rules hold.
use image::{Rgb, RgbImage};
use popupstory::book::{self, Options};
use popupstory::layout::{PAGE_H, PAGE_W};

/// A static scene (sky, ground, a tree) with a red figure walking across it.
fn frames(n: usize) -> Vec<RgbImage> {
    (0..n)
        .map(|i| {
            let fx = 60 + i as u32 * 6;
            RgbImage::from_fn(320, 180, |x, y| {
                if (fx..fx + 28).contains(&x) && (70..140).contains(&y) {
                    Rgb([214, 40, 40])
                } else if (40..52).contains(&x) && (60..120).contains(&y) {
                    Rgb([127, 79, 36])
                } else if y > 118 {
                    Rgb([106, 153, 78])
                } else {
                    Rgb([142, 202, 230])
                }
            })
        })
        .collect()
}

#[test]
fn builds_a_foldable_book() {
    let frames = frames(30);
    let mut plan = book::plan_keys(&frames, 6.0, Options { spreads: 2, ..Options::default() });
    assert_eq!(plan.spreads.len(), 2);
    book::analyze(&mut plan, &frames);

    let mut total = 0;
    for (i, sp) in plan.spreads.iter().enumerate() {
        // Folds never overlap and stay on the page.
        let mut steps: Vec<_> = sp.placed.iter().map(|p| p.step).collect();
        steps.sort_by(|a, b| a.0.total_cmp(&b.0));
        for w in steps.windows(2) {
            assert!(w[0].1 <= w[1].0, "folds overlap: {:?}", w);
        }
        for p in &sp.placed {
            assert!(p.step.0 >= 0.0 && p.step.1 <= PAGE_W);
            // Folds flat inside the closed book: depth + elevation + piece height ≤ page.
            assert!(p.d + p.e + p.hmm <= PAGE_H + 1e-3, "{} + {} + {} > page", p.d, p.e, p.hmm);
            // The box corner stays on the page when closed, and the piece's glued edge is on
            // the box's front face.
            assert!(p.d + p.h <= PAGE_H + 1e-3);
            assert!(p.e >= 0.0 && p.e < p.h && p.h <= p.e + p.hmm + 1e-3);
        }
        let key = frames[sp.key].clone();
        let out = book::render_spread(&plan, i, &key, &[]);
        assert!(out.base_svg.starts_with("<svg"));
        assert_eq!(out.pieces.len(), sp.placed.len());
        for p in &out.pieces {
            assert!(!p.outline.is_empty(), "every cut-out has a cut line");
        }
        total += out.pieces.len();
    }
    assert!(total > 0, "the walking figure should become at least one cut-out");
}
