# Visual verification

Check a built page against its frame, on your computer, with your own browser tools. This is
optional: FigCraft doesn't drive a browser, and no image leaves your computer through it.

## The loop

1. The agent calls `compare_design` for the frame, or reads its size from `design_context`. The result's
   `viewport` is the size to use. The frame's width and height are in CSS pixels.
2. It sizes the browser to that viewport and captures the page to a file in the project, for example
   with Playwright:

   ```ts
   await page.setViewportSize({ width: 1440, height: 900 });
   await page.screenshot({ path: ".figcraft-capture/home.png" });
   ```

3. It calls `compare_design {image: ".figcraft-capture/home.png"}`.
4. It fixes the layers listed in `differences`, and repeats from step 2.
5. It deletes the captures. Keep the folder out of version control.

## Reading the result

| Key | Meaning |
|---|---|
| `differences` | Layers that differ, largest first. Each difference is reported on the deepest layer it belongs to |
| `kind` | `position`, with `offset` in CSS pixels; `color`, with `expected` and `actual`; or `content` for anything else |
| `percent` | How much of the layer's own area differs |
| `note` | On text: check the font, size and line height first |
| `capture.sameSize` | Whether the page was the frame's size. Fix this first: a wrong height usually means the viewport or layout |
| `capture.density` | Device pixels per CSS pixel, worked out from the widths |
| `changedPercent` | How much of the frame differs. Don't treat it as a score |

Text is held to twice the tolerance, because fonts and antialiasing differ between renderers even
when the page is right. A clean result means the layers match within `tolerance`, not that the pages
are identical. Raise or lower `tolerance` (default 24 of 255) to suit.

## Limits

- Compares one frame at one size. For other sizes, compare with the frame marked for that size.
- Moves of more than 16 compared pixels show as `content`.
- Only what's visible in both images is compared. Scrolled-away content needs a full-page capture.
- `kind` is a best reading of the pixels, not a measurement of the code.
