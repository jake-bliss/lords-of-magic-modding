# Sprite review

Side-by-side review of IMP sprite frames, original against a 2x upscale, one verdict per unit.

```
# generate the pairs (needs magick and a Real-ESRGAN ncnn binary + models)
python3 tools/sprite-review/generate.py artifacts/sprite-review \
    --archive "<game>/imp.mpq" --listfile reports/member-names/all-profiles-imp-recovered.txt \
    --members MEMBERS.txt --esrgan PATH/realesrgan-ncnn-vulkan --models PATH/models

# review them
python3 tools/review-server.py artifacts/sprite-review --page tools/sprite-review --port 8778
```

`MEMBERS.txt` lists `.imp` member names one per line in the archive's own spelling.

## What the page shows

One unit per screen, its sequences down the page (`MOVE`, `STAND`, `MELEE_ATTACK`, `DEFEND`,
`GET_HIT`, `DIE`, `CORPSE`), each with **every facing** side by side — original row above, upscaled
row below, both drawn at the same physical size. `K` keeps, `X` rejects, `Space` held shows the
originals alone, `Z` cycles zoom. Verdicts persist to `artifacts/sprite-review/verdicts.json` on
every keypress.

`--frames-per-facing` defaults to **1**. Every facing is always covered; frames *within* one facing
are near-duplicates for judging image quality, and rendering all of them costs about three times as
much for no extra information. Raise it to inspect animation.

## 🔴 This makes pictures. It cannot make sprites.

Nothing here writes an `.imp`, and the reason is three measured format properties — see
[resolution and upscaling](../../docs/resolution-and-upscaling.md#%EF%B8%8F-the-same-upscaler-on-sprites-tried-and-it-is-a-different-problem).

- **IMP transparency is 1-bit.** A source frame has **2** distinct alpha values; the model's output
  has **240**. There is no alpha channel to hold them, so they must be thresholded back — throwing
  away the softened silhouette that is most of what the model added.
- **Palette index 1 is the shadow**, keyed by index rather than by colour (*Observed in gameplay
  2026-09-17*, [hotspots](../../docs/hotspots.md)). An RGB upscale interpolates it against its
  neighbours, which is exactly why the stipple smears in the output. It has to be lifted out as its
  own mask, scaled as a mask, and stamped back.
- **Hotspots move.** Placement is `top_left = anchor + placement - (w>>1, h>>1)`, so a doubled frame
  needs doubled anchor and hotspot records or the unit stands in the wrong place.

**So the review this feeds answers one narrow question** — *is the damage bad enough to be worth
fixing before the other 1,770 units* — and not "is this art good". Both defects are visible in the
output on purpose; hiding them would make the sample useless for the decision it exists to inform.

Portraits had none of these problems, which is why `tools/portrait-upscale/` ships art and this does
not: a portrait is an opaque rectangle of pure picture, a sprite is a cutout carrying indexed
semantics.
