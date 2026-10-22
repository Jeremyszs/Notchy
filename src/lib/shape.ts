// Outline of the island body, drawn in a box `w + 2 * ear` wide and `h` tall. The body is
// centred in the box. The ears flare outward at the top edge with concave quarter circles,
// the way a MacBook notch meets the screen, and the bottom corners stay convex.
//
// `bleed` extends the top of the box upward by that many px as a rectangle, so the black
// continues above the box top edge. The window clips at y=0, so the bleed is never seen,
// but it keeps black on screen if a transform ever lowers the top edge a few px.
export function islandPath(
  w: number,
  h: number,
  ear: number,
  bottomRadius: number,
  bleed = 0,
): string {
  const e = ear;
  const r = bottomRadius;
  const right = w + e;
  const box = w + 2 * e;

  return [
    `M0 ${-bleed}`,
    `H${box}`,
    `V0`,
    `A${e} ${e} 0 0 0 ${right} ${e}`,
    `V${h - r}`,
    `A${r} ${r} 0 0 1 ${right - r} ${h}`,
    `H${e + r}`,
    `A${r} ${r} 0 0 1 ${e} ${h - r}`,
    `V${e}`,
    `A${e} ${e} 0 0 0 0 0`,
    `Z`,
  ].join(" ");
}
