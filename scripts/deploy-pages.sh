#!/usr/bin/env bash
# Publish site/ and the web build (crates/rysmo_app/dist, see build-web.sh) to the gh-pages branch.
set -euo pipefail
cd "$(dirname "$0")/.."
dist=crates/rysmo_app/dist
[[ -f $dist/index.html ]] || { echo "No web build in $dist; run scripts/build-web.sh first." >&2; exit 1; }
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
cp site/index.html "$out/"
cp docs/screenshot.png "$out/"
cp crates/rysmo_app/brand/logo.svg crates/rysmo_app/brand/favicon.svg "$out/"
cp -R "$dist" "$out/editor"
touch "$out/.nojekyll"
remote=$(git remote get-url origin)
git -C "$out" init -q -b gh-pages
git -C "$out" add -A
git -C "$out" commit -q -m "Publish $(git rev-parse --short HEAD)"
git -C "$out" push -q -f "$remote" gh-pages
echo "Published to gh-pages."
