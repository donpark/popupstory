#!/usr/bin/env bash
# Rebuild the preloaded demo book (web/samples/big-buck-bunny.book.json) from the sample montage,
# with depth from Apple's Depth Pro 2 (H+). Two passes: the first picks the keyframes, Depth Pro 2
# runs once on all of them, the second builds the book with that depth.
#
#   DEPTH_PRO_2=/path/to/ml-depth-pro-2 scripts/build-demo.sh
#
# Needs: cargo, ffmpeg, uv, and a checkout of github.com/apple-aiml-research/ml-depth-pro-2.
# Depth Pro 2's weights are licensed for non-commercial research only (see its LICENSE_MODEL);
# the demo book is generated with it as a non-commercial demo. Use another depth model, or none,
# for anything else.
set -euo pipefail
cd "$(dirname "$0")/.."

: "${DEPTH_PRO_2:?set DEPTH_PRO_2 to your ml-depth-pro-2 checkout}"
MODEL="${MODEL:-depthpro2-h}"
VIDEO=web/samples/big-buck-bunny-montage.mp4
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cargo build --release
BIN=target/release/popupstory
ARGS=(--spreads 8 --keep-closeups --title "Big Buck Bunny")

echo "▸ pass 1: keyframes"
"$BIN" "$VIDEO" -o "$WORK/book" "${ARGS[@]}"

echo "▸ Depth Pro 2 ($MODEL) on $(ls "$WORK/book/keyframes" | wc -l | tr -d ' ') keyframes"
mkdir -p "$WORK/raw" "$WORK/depth"
(cd "$DEPTH_PRO_2" && uv run depth-pro-2 predict -i "$WORK/book/keyframes" -o "$WORK/raw" --save-png16 -m "$MODEL")
for f in "$WORK"/raw/*-depth16.png; do      # 16-bit inverse depth, white = near
  cp "$f" "$WORK/depth/$(basename "$f" -depth16.png).png"
done

echo "▸ pass 2: book with Depth Pro 2 depth"
"$BIN" "$VIDEO" -o "$WORK/book" "${ARGS[@]}" --depth-dir "$WORK/depth" --depth-label "Depth Pro 2" --embed-sheets
cp "$WORK/book/book.json" web/samples/big-buck-bunny.book.json
echo "✓ web/samples/big-buck-bunny.book.json ($(du -h web/samples/big-buck-bunny.book.json | cut -f1))"
