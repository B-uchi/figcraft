# Responsive design

A desktop frame alone can't say what a page does on a phone. FigCraft files have no breakpoints, so
Developer Connect gives agents only what you state outright, and tells them when you stated nothing.
It never works anything out from a frame's name or size.

## In FigCraft

Select a frame and open the **Inspect** tab, then **Screen sizes**:

1. Click **Mobile**, **Tablet** or **Desktop** to mark which version of a design the frame is. Click
   it again to clear it.
2. Give every version the same name under **Design it's a version of**.
3. Write rules under **Add a rule**, for example:
   - Mobile navigation collapses into a menu.
   - Sidebar becomes a drawer below 768 px.
   - Cards become a single-column list on mobile.
   - Hide this element below 640 px.

Rules are ordinary annotations in the **Responsive** category, so they show on the canvas, can be
edited under **Annotations**, and can be attached to any layer.

## What an agent receives

`design_context` returns the marked versions of the same design and the rules on and around the layer:

```json
"responsive": {
  "explicit": true,
  "versions": [{"group": "Home", "frames": [
    {"id": 41, "name": "Home phone", "viewport": "mobile", "width": 390, "height": 844},
    {"id": 12, "name": "Home large", "viewport": "desktop", "width": 1440, "height": 900}
  ]}],
  "rules": [{"id": 57, "layer": "Sidebar", "text": "Sidebar becomes a drawer below 768 px"}]
}
```

- A version's `width` is the size it was designed at, not a breakpoint.
- With nothing stated, `explicit` is `false` and `missing` says so. The agent should then ask or report
  what it assumed.
- Constraints, sizing and auto layout remain in each layer's `explicit` values, and say how it resizes.

## Limits

- Rules are plain text for the agent to read. FigCraft doesn't parse widths out of them.
- Marks live in layer data, so another app that drops layer data loses them.
