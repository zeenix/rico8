#!/usr/bin/env bash
# Build the example-cart index page for GitHub Pages: a shelf of
# cartridges, each linking to its playable web export. Kept as a script
# so the Pages build is reproducible locally:
#
#   ./.github/build-index.sh site
set -euo pipefail

site="${1:?usage: build-index.sh <site-dir>}"

# name:description pairs, in shelf order.
carts=(
  "hello:the canonical first cart"
  "sprite_move:sprite drawing, flipping, animation"
  "platformer:run, jump, collect coins"
  "map_demo:map scrolling and layer masks"
  "sfx_demo:a soundboard"
  "music_demo:starting and stopping a song"
)

cards=""
for entry in "${carts[@]}"; do
  name="${entry%%:*}"
  desc="${entry#*:}"
  title="${name//_/ }"
  cards+="  <a class=\"cart\" href=\"${name}.html\">
    <img src=\"${name}.png\" alt=\"${title} cartridge\">
    <span class=\"name\">${title}</span>
    <span class=\"desc\">${desc}</span>
  </a>
"
done

cat > "$site/index.html" <<EOF
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>rico-8 example carts</title>
<style>
  html, body { margin: 0; min-height: 100%; background: #000; }
  body { display: flex; flex-direction: column; align-items: center;
         font-family: monospace; color: #c2c3c7; padding: 24px 12px; }
  h1 { color: #fff1e8; font-size: 22px; margin: 8px 0 2px; }
  .stripe { display: flex; gap: 0; margin-bottom: 6px; }
  .stripe i { width: 22px; height: 6px; }
  p.sub { color: #5f574f; margin: 0 0 24px; }
  .shelf { display: flex; flex-wrap: wrap; gap: 24px;
           justify-content: center; max-width: 880px; }
  .cart { display: flex; flex-direction: column; align-items: center;
          gap: 6px; text-decoration: none; width: 176px; }
  .cart img { width: 160px; image-rendering: pixelated;
              transition: transform .1s; }
  .cart:hover img { transform: translateY(-6px); }
  .cart .name { color: #fff1e8; font-size: 14px; }
  .cart:hover .name { color: #ffec27; }
  .cart .desc { color: #5f574f; font-size: 11px; text-align: center; }
  footer { color: #5f574f; font-size: 11px; margin-top: 32px;
           text-align: center; line-height: 1.6; }
  a { color: #29adff; }
</style>
</head>
<body>
<h1>rico-8</h1>
<div class="stripe">
  <i style="background:#ff004d"></i><i style="background:#ffa300"></i>
  <i style="background:#ffec27"></i><i style="background:#00e436"></i>
  <i style="background:#29adff"></i><i style="background:#ff77a8"></i>
</div>
<p class="sub">example carts &mdash; click a cartridge to play</p>
<div class="shelf">
$cards</div>
<footer>
  arrows + z/x to play &middot; the cartridge images <em>are</em> the games:<br>
  save a .png and <code>load</code> it in the
  <a href="https://github.com/zeenix/rico8">rico-8 console</a>
</footer>
</body>
</html>
EOF

echo "wrote $site/index.html"
