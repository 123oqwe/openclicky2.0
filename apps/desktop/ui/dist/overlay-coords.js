// A plan speaks in screen points. The canvas speaks in device pixels measured
// from this window's own top-left corner, and that corner is not the screen's:
// macOS clamps a normal window below the menu bar, and a second display starts
// somewhere else entirely. So the transform carries the window origin instead of
// a constant being added to every coordinate.
(function (root, factory) {
  const api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.overlayCoords = api;
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
  function overlayTransform(dpr, originX, originY) {
    return { a: dpr, b: 0, c: 0, d: dpr, e: -originX * dpr, f: -originY * dpr };
  }

  return { overlayTransform };
});
