---
name: kata-print
description: Turn photos and cutouts into screen-print style artwork in a brand's own inks with Kata (tonal, spot, line plate, two-layer scenes). Use when someone wants risograph or screen-print looks, duotones, posterised photos, or photos recoloured to a brand palette.
---

# Printing in brand inks

## Choose inks from the system, never at random

- Start from the brand's inks and their roles. Pick 3–5 for a print: the dark
  `ink`, one or two `accent`s, and the `paper`.
- The ground of the poster must not be one of the subject's inks. If a
  subject's highlight ink equals the ground, that part of the body vanishes
  (an elbow disappears). Always pass `ground` to `kata_print` and fix any
  `ink_against_ground` finding by changing that tone's ink.
- Darkest ink: a deep shade of the poster's key colour reads richer than black.
- One "action" colour (the app's call-to-action gold, say) is spent on one
  element only, such as a "Coming soon" sticker.

## Choose the mode

- **tonal** (people, portraits, single objects): inks ranked dark to light.
  Add `prep: {"gamma": 0.72, "clahe": 40}` for backlit or dark-skinned
  subjects so features stay readable. Check faces with `kata_inspect faces`.
- **spot** (markets, crowds, anything whose point is its many colours): each
  patch gets its nearest ink by colour, so umbrellas stay distinct. Include an
  earthy ink if the ground is soil or road (red murram reads as terracotta).
- **line plate**: add `line: {"ink": "<dark>"}` when the photo contains
  drawing, writing or wire-thin detail the posterise would flatten.
- **two layers**: `background_spec` in pale inks plus `spec` in full inks with
  the subject mask: the city behind, quiet; the subject, loud.

## Region rules

Paper-coloured highlights on a face look like face paint. Add a `regions`
rule over each face: `forbid` the paper ink, `replace_with` the lightest skin
ink. Same idea for any area that must not take a given ink.

## Check

- `kata_view` the print at zoom 2 on faces and edges.
- If the subject reads as a flat blob, raise `clahe` or use one more ink.
- If it is noisy, raise `smooth` or `despeckle`.
