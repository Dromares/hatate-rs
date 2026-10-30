# Phase 6 contrast over the hosted-CI shot set (6.j, DAN-2223)

Measured on the shots the hosted `ci` run uploaded at head `b7500d5` (run 41073258779, GTK 4.14, 292 PNGs). Extreme-pixel method, WCAG 2.x, as `hatate-ryoku-mockup/DAN-2025/measure-gate.py.txt`. The scripts live outside the repo (the tree is Rust only): `worktrees/DAN-2223-tools/dan2025.py` and `dan2025b.py` (the DAN-2025 lines), `contrast.py` and `scanfam.py` (the label scan). Pixel coordinates are read off the 1100x800 queue shots and are in the scripts. Run: `python3 -I <script> <shot dir>`.

## 1. DAN-2025 lines, both schemes

| line | element | scheme | fg | bg | ratio | floor | verdict | shot |
|---|---|---|---|---|---|---|---|---|
| 1.3 | Unsearched status text | dark | #8c8680 | #141414 | 5.12 | 4.5 | PASS | queue-dark.png |
| 2.1 | menubar item at rest (worst of 5: Files) | dark | #8a847e | #0d0d0d | 5.26 | 4.5 | PASS | queue-dark.png |
| 2.7 | empty state text "nothing queued" | dark | #8a847e | #0d0d0d | 5.26 | 4.5 | PASS | empty-dark.png |
| 2.7 | empty state text "Or add them from a folder, or pull" | dark | #8c8680 | #141414 | 5.12 | 4.5 | PASS | empty-dark.png |
| 2.8 | review error text "The local picture could not be ope" | dark | #8a847e | #0d0d0d | 5.26 | 4.5 | PASS | review-error-dark.png |
| 2.8 | review error text "<tmp>/synthetic/review-0.png: ..." | dark | #8a847e | #0d0d0d | 5.26 | 4.5 | PASS | review-error-dark.png |
| 1.3 | Unsearched status text | light | #5f5b53 | #f2ede1 | 5.78 | 4.5 | PASS | queue-light.png |
| 2.1 | menubar item at rest (worst of 5: Files) | light | #5c574f | #e8e1d2 | 5.50 | 4.5 | PASS | queue-light.png |
| 2.7 | empty state text "nothing queued" | light | #5c574f | #e8e1d2 | 5.50 | 4.5 | PASS | empty-light.png |
| 2.7 | empty state text "Or add them from a folder, or pull" | light | #5f5b53 | #f2ede1 | 5.78 | 4.5 | PASS | empty-light.png |
| 2.8 | review error text "The local picture could not be ope" | light | #5c574f | #e8e1d2 | 5.50 | 4.5 | PASS | review-error-light.png |
| 2.8 | review error text "<tmp>/synthetic/review-0.png: ..." | light | #5c574f | #e8e1d2 | 5.50 | 4.5 | PASS | review-error-light.png |
| 1.4 | sort chevron (glyph) vs header | dark | #cdc4ba | #141414 | 10.70 | 3.0 | PASS | queue-sorted-dark.png |
| 2.3 | thumb frame vs row 1 bg | dark | #7b7670 | #141414 | 4.09 | 3.0 | PASS | queue-sorted-dark.png |
| 2.3 | thumb frame vs row 2 bg | dark | #7b7670 | #141414 | 4.09 | 3.0 | PASS | queue-sorted-dark.png |
| 2.4 | v-scrollbar thumb vs trough/row | dark | #7f7a74 | #141414 | 4.33 | 3.0 | PASS | queue-sorted-dark.png |
| 2.4 | v-scrollbar thumb vs page | dark | #7f7a74 | #0d0d0d | 4.57 | 3.0 | PASS | queue-sorted-dark.png |
| 2.4 | h-scrollbar thumb vs row | dark | #7f7a74 | #141414 | 4.33 | 3.0 | PASS | queue-sorted-dark.png |
| 2.4 | h-scrollbar thumb vs page | dark | #7f7a74 | #0d0d0d | 4.57 | 3.0 | PASS | queue-sorted-dark.png |
| 1.1 | focus ring in list (bbox (35, 330, 1047, 383)) vs outside (page/zebra) | dark | #cdc4ba | #0d0d0d | 11.29 | 3.0 | PASS | queue-focus-dark.png |
| 1.1 | focus ring in list (bbox (35, 330, 1047, 383)) vs inside (row) | dark | #cdc4ba | #141414 | 10.70 | 3.0 | PASS | queue-focus-dark.png |
| 1.4 | sort chevron (glyph) vs header | light | #211d17 | #f2ede1 | 14.35 | 3.0 | PASS | queue-sorted-light.png |
| 2.3 | thumb frame vs row 1 bg | light | #686256 | #f2ede1 | 5.18 | 3.0 | PASS | queue-sorted-light.png |
| 2.3 | thumb frame vs row 2 bg | light | #686256 | #f2ede1 | 5.18 | 3.0 | PASS | queue-sorted-light.png |
| 2.4 | v-scrollbar thumb vs trough/row | light | #706c64 | #f2ede1 | 4.47 | 3.0 | PASS | queue-sorted-light.png |
| 2.4 | v-scrollbar thumb vs page | light | #706c64 | #e8e1d2 | 4.01 | 3.0 | PASS | queue-sorted-light.png |
| 2.4 | h-scrollbar thumb vs row | light | #706c64 | #f2ede1 | 4.47 | 3.0 | PASS | queue-sorted-light.png |
| 2.4 | h-scrollbar thumb vs page | light | #706c64 | #e8e1d2 | 4.01 | 3.0 | PASS | queue-sorted-light.png |
| 1.1 | focus ring in list (bbox (35, 330, 1047, 383)) vs outside (page/zebra) | light | #211d17 | #e8e1d2 | 12.88 | 3.0 | PASS | queue-focus-light.png |
| 1.1 | focus ring in list (bbox (35, 330, 1047, 383)) vs inside (row) | light | #211d17 | #f2ede1 | 14.35 | 3.0 | PASS | queue-focus-light.png |
| F-B3 | survived glyph border (x=523,y=329) vs panel | dark | #474542 | #141414 | 1.93 | 3.0 | FAIL | survived-dark.png |
| F-B3 | survived glyph border (x=523,y=331) vs panel | dark | #474542 | #141414 | 1.93 | 3.0 | FAIL | survived-dark.png |
| F-B3 | survived glyph border vs page | dark | #474542 | #0d0d0d | 2.03 | 3.0 | FAIL | survived-dark.png |
| F-B3 | survived glyph border (x=523,y=329) vs panel | light | #938f86 | #f2ede1 | 2.76 | 3.0 | FAIL | survived-light.png |
| F-B3 | survived glyph border (x=523,y=331) vs panel | light | #938f86 | #f2ede1 | 2.76 | 3.0 | FAIL | survived-light.png |
| F-B3 | survived glyph border vs page | light | #938f86 | #e8e1d2 | 2.48 | 3.0 | FAIL | survived-light.png |

Line 2.2 (`review-compare-differences-{dark,light}.png`, image pane box (30,180)-(870,488)): pixels with R != G or G != B: **0 in dark, 0 in light**; max channel spread 0. The two 2.2 measurements are counts, not ratios: the Differences image pane has no pixel with R != G or G != B in either scheme, so the pane is zero chroma (PASS, both schemes).

## 2. Findings

- **F-B3 FAILS** (3:1 non-text floor): the `.survived-glyph` box border on the Nothing survived page is `ink_28`, 1.93:1 against the panel and 2.03:1 against the page in dark, 2.76:1 and 2.48:1 in light. Python draws it at `ink_46`. Not fixed in 6.j (no riders, DAN-2421). **Fixed in the 6.j follow-ups (DAN-2472): the box and every popover edge are `ink_46` now, measured in section 5.**
- Lines **1.5**, **2.5**, **2.6**, **2.9** are not contrast ratios. 2.5: all 102 shots whose surface states a minimum are `FIT` by PNG header at the CI toolkit (`index.tsv` of run 41082758250, 0 `OVER`, 0 `UNDER`). Four of the 102 are the `parser-health*-min-*` shots: Parser Health was a dialog with no enforced minimum; its 760 x 460 is the size Python resizes it to (`resize(760, 460)`), which `shot --all` states and checks and, since DAN-2472, the dialog enforces with `set_size_request` (section 5). 2.6 (Cancel is the default) is pinned by the smoke tests cited in section 7 of `docs/phase6-coverage.md`. 1.5 (inactive selection needs a real compositor backdrop) and 2.9 (crop marks, ghost kanji parity) go to Oderisi's read.
- Line **1.2** (status tiers, six glyphs) is a look judgement, owner Oderisi.

## 3. Label scan (triage list, not verdicts)

`contrast.py` measures every label box of every shot (`*.rects`, 11,176 boxes, disabled labels included; boxes with no ink skipped). Raw output: `worktrees/DAN-2223-contrast-scan.txt`.

| scheme | state | boxes | minimum | below 4.5:1 |
|---|---|---|---|---|
| dark | on | 5350 | 1.36:1 | 754 |
| dark | disabled | 204 | 1.31:1 | 182 |
| dark | selected | 4 | 7.86:1 | 0 |
| light | on | 5410 | 1.83:1 | 527 |
| light | disabled | 204 | 1.97:1 | 182 |
| light | selected | 4 | 5.88:1 | 0 |

Every `on` label below 4.5:1 is one of two things (`scanfam.py` lists the remainder, which is empty):
- the **em-dash placeholder** in an empty cell (`—`, 3.02:1 light and 3.09:1 dark). It clears 3:1 as a graphic and fails 4.5:1 as text. **Design call for Oderisi:** is a placeholder dash text?
- labels **behind a modal scrim** (`dialog`, `dialog-nolabel`, `queue-remove`): dimmed on purpose, not read while the dialog is open.

The `disabled` labels below 4.5:1 (182 per scheme) are WCAG-exempt inactive text, listed so nothing is passed by omission.

## 4. Not judgeable from shots (owner)

- Window chrome, GPU renderer (`GSK_RENDERER=ngl`), real-compositor inactive selection, HiDPI, hover wash: **Oderisi / board**, as DAN-2025's list.
- The `review-picker` popover is its own surface, so the Tab walker cannot reach it: its Tab stops are **not judged** (section 8 of the coverage doc). Owner: Oderisi, from `review-picker-*.png`.

## 5. 6.j follow-ups (DAN-2472, PR #84)

Measured on the hosted-CI shots of head `c0ff4a9` (run 41117309295, GTK 4.14, 359 index rows, 0 non-FIT), extreme-pixel method, script `worktrees/DAN-2472-tools/measure.py` (outside the tree), output `worktrees/DAN-2472-evidence/measure.md`, `glyph.md`, `found-ring.md`, PNG headers in `headers.md`. Both schemes.

| element | dark | light | floor | shot |
|---|---|---|---|---|
| popover and menu edge (`ink_46`) vs the popover fill | 3.19 | 3.27 | 3.0 | `menu-*`, `queue-menu-*`, `review-picker*-popover.png` |
| popover and menu edge vs the page behind | 3.36 | **2.93** | 3.0 | same; light is below the floor |
| glyph box top edge vs card | 3.19 | 3.27 | 3.0 | `survived-{dark,light}.png` |
| site-list scrollbar thumb vs page | 4.38 | 4.27 | 3.0 | `parser-health-{,bottom-}{dark,light}.png` |
| picker current-candidate bar vs popover | 10.70 | 14.35 | 3.0 | `review-picker-*.png` |
| picker keyboard row (stamp) vs popover | 10.70 | 14.35 | 3.0 | `review-picker-kb-*.png` |
| picker stamp text | 11.29 | 14.88 | 4.5 | `review-picker-kb-*.png` |
| Parser Health Close, focus ring inside its own box | 11.29 | 12.88 | 3.0 | `parser-health-kb-*.png` |
| Parser Health `Check every site now` (insensitive) | 3.07 | 3.09 | 4.5 | all Parser Health states; WCAG-exempt inactive control, listed so it is not passed by omission |

- The `ink_46` edge against the **light page** is 2.93:1: the token itself, not a wrong tier (Python draws these edges at `ink_46`). A stronger edge needs a different token, a design call for Oderisi.
- The dash row was not changed: all seven placeholder dashes are `CellInk::Ink65` (`queue_view.rs:996`). A 1 px sans dash straddles two pixel rows, so its extreme pixel reads about `ink_45`; that is rasterisation, not the tier.
- Parser Health is 760 x 460 in every state at the hosted-CI toolkit (`headers.md`, 0 non-FIT). Its list takes no Tab stop (`Close` is the only one); the list scrolls on Up, Down, Page Up, Page Down, Home and End.
- **Item 8, decided (Cloud, DAN-2458):** the Differences letterbox in light stays the panel colour (`#f2ede1`), not `#1a1a1a`. A black band on a light panel would be the loudest object on screen. The pane itself stays zero chroma (section 1, line 2.2).

Assertions that pin these (each goes red when its line is reverted, `worktrees/DAN-2472-revert.log`): the 760 x 460 floor `tests/smoke.rs:9753`, the persistent bar `tests/smoke.rs:9766`, the non-focusable cells `tests/smoke.rs:9807`, Close the only Tab stop (the scroller is not a stop) `tests/smoke.rs:9813`, the glyph box edge `src/theme.rs:1026`, the popover edge `src/theme.rs:1032`, the picker stamp rows `src/theme.rs:1060` and `:1061`, the dash tier `src/queue_view.rs:996`.
