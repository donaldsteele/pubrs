# M25: WordArt, shadows and BorderArt

What works:

- **WordArt.** A frame kind of its own: one line of text warped as a whole. Warps: straight, arch up, arch down,
  circle, wave, slant up, slant down, inflate. The fill is a colour, a swatch or a gradient (the gradient spans the
  letters), with an optional outline and shadow. The text is scaled down (never up) to fit the frame; `size` is the
  nominal size. In the PDF the letters are vector outlines, not text and not an image; a tagged PDF still carries
  the words as `ActualText`.
- **Shadows.** Any frame (shape, picture, text frame, WordArt) can have a drop shadow: offset, blur, colour and
  opacity. In PNG it is a deterministic blur (three box passes each way, integers only); in PDF it is a 150 dpi
  RGBA image with a soft mask, and PDF/X-4 and PDF/A-2b exports stay clean.
- **BorderArt.** Twelve original CC0 patterns (stars, hearts, dots, waves, zigzag, diamonds, leaves, ropes, checks,
  scallops, arrows, snowflakes), each a tile and a corner, tiled along the inside of a frame with a whole number of
  tiles per edge. Text inside a bordered text frame stays clear of the band.
- **Find and replace.** `text_find` and Replace All also reach WordArt text.
- **MCP.** `frame_add_wordart`, `wordart_set`, `border_art_list`; `frame_set_style` takes `shadow` and
  `border_art` (an object, or `"none"`).
- **GUI.** Insert ▸ WordArt… (gallery of twelve styles with live previews), Format ▸ Shadow, Format ▸ Border Art…
  (picker with previews), and Shadow, Border art and WordArt sections in the properties panel.
- **Schema 9.** `v8_to_v9` stamps the version; `tests/fixtures/schema_v9.pubrs` holds WordArt, both kinds of shadow
  and a border. Design notes: ADR 0020.

Goldens: `wordart_warps` (all eight warps), `shadows`, `borderart_stars` and `borderart_ropes`. Run
`cargo test -p pubrs-render --test wordart` and `cargo test -p pubrs-io --test render_golden`.

Try it:

  cargo run -p pubrs-app            # Insert > WordArt..., pick "Arch", then change Shape to Wave in the panel

  frame_add_wordart {"text": "Grand Opening", "x": 36, "y": 36, "width": 400, "height": 140, "warp": "arch_up",
                     "fill": {"gradient": "linear", "angle": 90, "stops": [{"offset": 0, "color": "#FACC15"},
                     {"offset": 1, "color": "#DC2626"}]}, "outline": "#1E3A5F", "shadow": {"blur": 4}}
  frame_set_style {"frame_id": "...", "border_art": {"pattern": "stars", "width": 18}, "shadow": {"blur": 6}}
