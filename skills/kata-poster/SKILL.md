---
name: kata-poster
description: Art-direct and build social posters and magazine-style covers with Kata, from brief to posting plan. Use when someone asks for posters, covers, promo graphics, campaign posts or launch teasers built from their own photos, with their brand's colours and logo, in language their audience actually speaks.
---

# Making posters with Kata

A poster is right when someone in the audience stops scrolling, gets it in two
seconds and would repost it. Everything below serves that. Work in this order;
do not skip the looking.

## 1. Brief and brand (before any pixels)

- Load the brand with `kata_brand`. If there is no brand file, build one first
  (skill `kata-brand`). Inks have **roles**; respect them.
- Write down, in one line each: who it is for, what it should make them do,
  and the one feature or feeling this poster carries. One poster, one message.
- Ask what is not allowed: subjects to avoid, places that are sensitive
  (places of worship stay away from nightlife and drink), people who have not
  agreed to appear.

## 2. Research, then a concept per poster

- Look at real print culture, not other apps. Borrow a *structure*, not a
  costume: a magazine masthead and cover lines; a kanga's border, centre and
  saying; a timetable grid; an LP's obi strip and tracklist; a hand-painted
  shop sign. Say in one sentence what you borrowed and why it fits the place.
- If the audience mixes cultures, let each borrowing come from a tradition that
  actually lives in that city, then translate it into the brand's inks and type
  so the series still reads as one title.
- Prefer one strong idea per poster over decoration. No tape, no fake stickers
  on everything, no boxes around text "for contrast". If it is a gimmick, cut it.

## 3. Copy that sounds like people

- Write how the audience talks to friends, not how brands talk. Use local
  words where people really use them (Swahili or Sheng in Nairobi: "wikendi
  hii", "bei ya mwisho?", "gumzo"), spelled the way locals spell them.
- Headline: 2 to 6 words, a feeling or a question. Deck: one sentence that says
  what the product does *for them*. Check the deck conveys the actual feature;
  if a stranger would not know what happens next, rewrite it.
- Read every line against the product: never say "you don't need a plan" on a
  planning app. Check plurals and spelling in every language.
- Captions: natural, a little playful, one emoji at most, a question or a tag
  prompt at the end. Three or four hashtags.

## 4. The picture

1. `kata_segment` the photo. In a busy scene pass a `box` hint around the
   subject. Trust the raw mask; look at it with `kata_view` (zoom 2, checker)
   before changing anything. Follow skill `kata-cutout`.
2. Print it with `kata_print` (skill `kata-print`): tonal for people, spot for
   colourful scenes, `background_spec` for "the subject in its street".
3. Always pass `ground` so a limb printed in the ground colour is caught.

## 5. Layout (a scene for `kata_compose`)

- Canvas from the target: `ig-portrait` 1080x1350 for feed, `ig-square` for
  covers, `ig-story` 1080x1920. Keep text, logo and stickers inside safe areas.
- One margin everywhere (about 5–6% of the width). Headline, deck and logo
  share one left edge. Type hierarchy: masthead, headline, deck, small labels.
- Subjects run off an edge (usually the bottom) instead of floating, unless the
  concept frames them (a medallion, a stamp).
- Logo: the transparent file, never on a plate or pill. Full colour only on
  dark grounds; on light or saturated grounds use a one-colour version in the
  poster's dark ink (`logo` layer with `color`). Width 10–22% of the canvas.
  One sign-off: logo with the web address set small beneath it, same edge.
- Finish: `finish.paper` 0.6–0.9 and `finish.halftone` around 0.1 so the series
  feels printed. Generated paper only; check any scanned texture with
  `kata_texture check`.

## 6. Review, fix, repeat (skill `kata-review`)

`kata_compose` returns a review. Fix every error. Then look yourself with
`kata_view`: every headline edge, the logo at zoom 2, the subject's outline,
the whole poster with `boost` for hidden lines. Only then show it.

## 7. Series and posting plan

- Number issues in the order people will see them; the launch or reveal comes
  last. Keep masthead, margins and finish identical across the series.
- Suggest a cadence (for example three posts a week, evenings 18:00–20:00 and
  weekend late mornings), what to post first (atmosphere before product), and
  one caption per post.
- Flag what needs a human: permission from people in photos, photographer
  credit, anything sensitive.
