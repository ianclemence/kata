---
name: kata-review
description: Review posters, covers and cutouts the way a demanding art director would before anything is posted, using Kata's checks and close-up views. Use before calling any visual design finished, and whenever someone says a design looks off, sloppy or "AI-made".
---

# Review before you show it

People notice what you skipped. Assume there is a problem until you have looked.

## Automatic checks (read every finding)

`kata_compose` returns a review. Errors must be fixed. Warnings need a look
with `kata_view` on the crop it points to. Checks and what they mean:

| Check | What went wrong | Usual fix |
|---|---|---|
| `text_collision` | type runs into a picture, logo or sticker | move the type or the art; never shrink the art to fit |
| `text_overlap` | two text elements overlap | re-space the stack |
| `legibility` | contrast too low against what is really behind it | darker ink, or the one-colour logo |
| `logo_size` | logo outside the brand's size range | resize to 10–22% of width |
| `safe_area` | important element under the platform's buttons | move inside the margins |
| `target_size` | wrong canvas for the platform | compose at the target size |
| `ink_against_ground` | part of the subject printed in the ground colour | change that ink |
| `mask_sliver` / `mask_island` | furniture or specks left in a cutout | `remove` polygon / `drop_specks` |
| `dark_region` | faces printed too dark | `prep` gamma and clahe |
| `texture_crease` | the paper scan has a fold line | use generated paper |

## Look yourself, every time

1. Whole poster at normal size: does it read in two seconds? Is there one
   clear thing to look at?
2. `kata_view` with `boost: true` on the whole poster: hidden lines, banding,
   seams in textures show up as bright or dark streaks.
3. Zoom 2 on: the logo; every headline edge (letters cut off, bleeding past
   the edge); where text meets the subject; the subject's outline (halos,
   slivers, missing limbs, elbows that vanished).
4. Check the copy: spelling in every language, issue numbers in sequence, the
   deck says what the product does.
5. Compare against the rest of the series: same margins, masthead, finish.

## When the person points something out

Treat it as a pattern, not a one-off: if one logo was too big, check all of
them; if one face was too dark, check every face in the series. Then say
exactly what changed.
