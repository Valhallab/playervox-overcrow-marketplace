import test from "node:test";
import assert from "node:assert/strict";

import * as siteModule from "../../web/landing/site-7ef11cd4.js";

const { getCanvasSize, selectEffectsMode } = siteModule;

test("animated effects are used only when motion, data, and WebGL allow them", () => {
  assert.equal(
    selectEffectsMode({
      supportsWebGL: true,
      reducedMotion: false,
      saveData: false,
    }),
    "animated",
  );

  for (const constraints of [
    { supportsWebGL: false, reducedMotion: false, saveData: false },
    { supportsWebGL: true, reducedMotion: true, saveData: false },
    { supportsWebGL: true, reducedMotion: false, saveData: true },
  ]) {
    assert.equal(selectEffectsMode(constraints), "static");
  }
});

test("canvas sizing caps high-density mobile displays", () => {
  assert.deepEqual(getCanvasSize(375, 812, 3), {
    width: 562,
    height: 1218,
    pixelRatio: 1.5,
  });
});

test("canvas sizing stays within the GPU pixel budget on large displays", () => {
  const result = getCanvasSize(3840, 2160, 2);

  assert.ok(result.width * result.height <= 3_000_000);
  assert.ok(result.pixelRatio < 1);
  assert.ok(result.width > 0);
  assert.ok(result.height > 0);
});

test("the JavaScript bundle has no command-palette behavior", () => {
  assert.equal("isPaletteShortcut" in siteModule, false);
});
