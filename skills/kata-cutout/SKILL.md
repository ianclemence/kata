---
name: kata-cutout
description: Cut people and objects out of photos cleanly with Kata's matting model, hints and explicit mask edits. Use when someone needs a transparent cutout, a sticker, a subject separated from a busy background, or a mask fixed (stray furniture, holes, hair).
---

# Clean cutouts

## The rule that matters most

The model's raw mask is usually the best version. Do not "improve" a clean cut.
Every edit you make is visible to the person; edits that guess (filling every
hole, smoothing everything) make cutouts worse. Edit only what you can point to.

## Steps

1. `kata_status`: confirm ONNX Runtime and a model are present.
2. `kata_segment` with the photo.
   - Busy scene (a vehicle in traffic, one person in a crowd): pass a `box`
     hint around the subject. The model then sees mostly the subject and locks
     onto it. Keep the default margin.
   - Two subjects and you want one: a `points` hint on the one you want.
3. Look before editing: `kata_view` the cutout with `checker: true`, then zoom 2
   on hair, hands, the gap between arms and body, and where the subject touches
   furniture or the ground.
4. Read the findings that came back:
   - `mask_island`: a separate piece. Keep it only if it belongs (a held phone,
     a hat brim). Otherwise `drop_specks` or `keep_largest`.
   - `mask_sliver`: a thin strip attached to the subject. Usually a chair leg,
     railing or table edge. Remove it with a `remove` polygon traced on the
     real edges, not a box.
5. Edit with `kata_refine_mask`, smallest edit first:
   - `fill_holes` with a small `max_area` (a few hundred px). Gaps between an
     arm and a body are background: never fill those.
   - `remove` / `add` with polygons that follow real edges.
   - `matte` (with the image) for hair and fur edges.
   - `feather` 1–2 px only if the edge looks cut with scissors.
6. If an edit made it worse, drop that op and re-apply the list. Edits are a
   list precisely so they can be undone.

## Things that go wrong

- Faces too dark after printing: that is the print step, not the cut. See
  `kata-print`.
- A person's lower body cut off by the frame: the cut is fine; place the
  subject so it runs off the bottom edge of the poster.
- Number plates and private details: blur them before printing.
