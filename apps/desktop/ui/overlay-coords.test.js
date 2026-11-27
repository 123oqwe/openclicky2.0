const test = require('node:test');
const assert = require('node:assert');

const { overlayTransform } = require('./dist/overlay-coords.js');

// The machine #140 was measured on: a retina display, so one logical point is two
// device pixels, and macOS clamped the overlay window to y=33.
const DPR = 2;
const RING_CENTRE = { x: 756, y: 490 };
const TOLERANCE_PT = 2;

function canvasPixel(point, origin) {
  const t = overlayTransform(DPR, origin.x, origin.y);
  return {
    x: t.a * point.x + t.c * point.y + t.e,
    y: t.b * point.x + t.d * point.y + t.f,
  };
}

// The canvas starts at the window's top-left corner, and the window sits at
// `origin` on the display. This is the part the transform has to agree with.
function screenPoint(pixel, origin) {
  return { x: origin.x + pixel.x / DPR, y: origin.y + pixel.y / DPR };
}

test('the ring lands on the same screen point wherever the window sits', () => {
  for (const origin of [
    { x: 0, y: 33 },
    { x: 200, y: 150 },
    { x: -1512, y: 0 },
  ]) {
    const seen = screenPoint(canvasPixel(RING_CENTRE, origin), origin);
    assert.ok(
      Math.abs(seen.x - RING_CENTRE.x) <= TOLERANCE_PT,
      `x drifted to ${seen.x} for window origin ${JSON.stringify(origin)}`,
    );
    assert.ok(
      Math.abs(seen.y - RING_CENTRE.y) <= TOLERANCE_PT,
      `y drifted to ${seen.y} for window origin ${JSON.stringify(origin)}`,
    );
  }
});

test('a clamps-below-the-menu-bar window is not the same as one at zero', () => {
  const atZero = canvasPixel(RING_CENTRE, { x: 0, y: 0 });
  const clamped = canvasPixel(RING_CENTRE, { x: 0, y: 33 });
  const movedPx = (clamped.y - atZero.y) * -1;
  assert.strictEqual(
    movedPx / DPR,
    33,
    'the transform has to move by the origin; a zero offset here is the #140 bug',
  );
});
