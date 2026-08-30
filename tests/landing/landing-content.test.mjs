import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

const sourceRoot = process.argv[2]
  ? pathToFileURL(`${resolve(process.argv[2])}/`)
  : new URL("../../web/landing/", import.meta.url);
const html = await readFile(new URL("index.html", sourceRoot), "utf8");
const css = await readFile(
  new URL("styles-5428c51a.css", sourceRoot),
  "utf8",
);

const demoDiscord = html.match(
  /<article class="demo-widget demo-discord">([\s\S]*?)<\/article>/,
)?.[1] ?? "";
const demoPerformance = html.match(
  /<article class="demo-widget demo-performance">([\s\S]*?)<\/article>/,
)?.[1] ?? "";

test("the proprietary product page contains no GitHub or open-source claims", () => {
  assert.doesNotMatch(html, /github\.com|open[ -]source/i);
});

test("the fake overlay uses designed controls instead of text glyph stand-ins", () => {
  assert.doesNotMatch(html, /class="feature-icon"|class="layout-cursor"/);
  assert.doesNotMatch(html, />Ⅱ<|>│◀<|>▶│</);
  assert.match(html, /class="dock-button"/);
});

test("the work-in-progress page does not expose launch-only sections or actions", () => {
  assert.match(html, /Coming soon/i);
  assert.match(html, /content="OverCrow is an upcoming, native/i);
  assert.doesNotMatch(
    html,
    /Available now|Private by default|id="compatibility"|id="install"|class="signal-row"|data-copy=|Install OverCrow|Get OverCrow/i,
  );
});

test("mission rows and checklist markers share an even aligned size", () => {
  assert.match(
    css,
    /\.mission-group > p i,\s*\.notes-widget li i\s*{[^}]*width:\s*10px;[^}]*height:\s*10px;/s,
  );
  assert.match(css, /\.mission-group > p\s*{[^}]*line-height:\s*10px;/s);
  assert.match(css, /\.notes-widget li\.done i::after\s*{[^}]*content:\s*"";/s);
});

test("header and footer use the PlayerVox dual-mark hover animation", () => {
  assert.equal((html.match(/class="brand-mark-fill"/g) ?? []).length, 2);
  assert.equal((html.match(/brand-mark-image brand-mark-dark/g) ?? []).length, 2);
  assert.equal((html.match(/brand-mark-image brand-mark-light/g) ?? []).length, 2);
  assert.match(
    css,
    /\.brand-link:hover \.brand-mark-fill,[^{]*\.brand-link:focus-visible \.brand-mark-fill\s*{[^}]*transform:\s*translateY\(-110%\);/s,
  );
  assert.match(
    css,
    /\.brand-link:hover \.brand-mark-light,[^{]*\.brand-link:focus-visible \.brand-mark-light\s*{[^}]*opacity:\s*1;/s,
  );
  assert.doesNotMatch(css, /0 0 0 1px rgba\(200, 255, 92/);
});

test("the focused landing page has no command palette", () => {
  assert.doesNotMatch(html, /Quick look|Ctrl K|data-command-trigger|id="command-menu"/i);
  assert.doesNotMatch(css, /\.command-trigger|\.command-menu|\.command-panel/);
});

test("Discord voice is part of both overlay layouts", () => {
  assert.equal((html.match(/class="overlay-widget discord-widget"/g) ?? []).length, 1);
  assert.match(html, /<strong>DISCORD VOICE<\/strong>/);
  assert.match(css, /\.discord-widget\s*{[^}]*top:\s*3\.5%;[^}]*left:\s*3%;/s);
  assert.match(
    css,
    /@media \(max-width:\s*901px\)[\s\S]*?\.discord-widget\s*{[^}]*grid-column:\s*1\s*\/\s*-1;[^}]*grid-row:\s*1;/s,
  );
});

test("overlay widgets use real brand, avatar, state, and media visuals", () => {
  assert.match(
    html,
    /class="discord-logo" src="\/discord-symbol-blurple\.png"/,
  );
  assert.equal((html.match(/<img class="voice-avatar avatar-/g) ?? []).length, 3);
  assert.match(html, /src="\/avatar-mika\.png"/);
  assert.match(html, /src="\/avatar-noct\.png"/);
  assert.match(html, /src="\/avatar-lin\.png"/);
  assert.doesNotMatch(html, /voice-activity/);
  assert.doesNotMatch(css, /\.voice-activity/);
  assert.match(
    css,
    /\.voice-member\.speaking > \.voice-avatar\s*{[^}]*border-color:\s*var\(--lime\);/s,
  );
  assert.match(html, /class="voice-state-icons"/);
  assert.match(html, /aria-label="Muted"/);
  assert.match(html, /aria-label="Deafened"/);
  assert.doesNotMatch(html, /class="widget-glyph/);
  assert.doesNotMatch(html, /class="media-bars"/);
  assert.match(html, /class="media-cover"/);
});

test("Discord mute and deafen states use the official Lucide icon geometry", () => {
  assert.match(html, /class="lucide lucide-mic-off"/);
  assert.match(html, /class="lucide lucide-headphone-off"/);
  assert.match(html, /<path d="M12 19v3" \/>/);
  assert.match(html, /<path d="M21 14h-1\.343" \/>/);
});

test("the fake overlay keeps small widget text out of transformed layers", () => {
  const appWindowRule = css.match(/\.app-window\s*{[^}]*}/s)?.[0] ?? "";

  assert.doesNotMatch(appWindowRule, /transform:/);
  assert.doesNotMatch(
    css,
    /html\[data-effects="animated"\] \.overlay-widget\s*{[^}]*animation:/s,
  );
  assert.doesNotMatch(css, /@keyframes widget-float/);
});

test("the mobile session row has no helper copy or reserved dead height", () => {
  assert.doesNotMatch(html, /Started with game process/);
  assert.match(
    css,
    /@media \(max-width:\s*901px\)[\s\S]*?\.session-widget,\s*\.status-widget\s*{[^}]*min-height:\s*88px;/s,
  );
});

test("the compose-space demo overlays three real widget samples on a game scene", () => {
  assert.match(
    html,
    /class="layout-game-scene" src="\/overlay-demo-game\.webp"/,
  );
  assert.equal((html.match(/class="demo-widget demo-/g) ?? []).length, 3);
  assert.match(html, /class="demo-widget demo-discord"/);
  assert.match(html, /class="demo-widget demo-performance"/);
  assert.match(html, /class="demo-widget demo-twitch"/);
  assert.doesNotMatch(html, /class="layout-grid"|class="layout-chip/);
});

test("compose-space widgets preserve the passive game view", () => {
  assert.match(css, /\.demo-widget\s*{[^}]*background:\s*transparent;/s);
  assert.match(css, /\.demo-widget\s*{[^}]*border:\s*0;/s);
  assert.match(css, /\.demo-widget\s*{[^}]*box-shadow:\s*none;/s);
  assert.doesNotMatch(css, /\.demo-widget\s*{[^}]*backdrop-filter:/s);
});

test("the compose-space Discord sample shows participant voice state", () => {
  assert.doesNotMatch(demoDiscord, /class="demo-widget-title"/);
  assert.match(demoDiscord, /class="speaking"/);
  assert.match(demoDiscord, /class="demo-voice-state-icons"/);
  assert.match(demoDiscord, /class="lucide lucide-mic-off"/);
  assert.match(demoDiscord, /class="lucide lucide-headphone-off"/);
});

test("compose-space voice and chat copy stays subordinate to the game", () => {
  assert.match(
    css,
    /\.layout-demo \.demo-voice-list p\s*{[^}]*grid-template-columns:\s*20px max-content 1fr;[^}]*font-size:\s*10px;/s,
  );
  assert.match(
    css,
    /\.layout-demo \.demo-chat-lines p\s*{[^}]*font-size:\s*10px;/s,
  );
});

test("the compose-space Performance sample matches the four-headerless KPI layout", () => {
  assert.doesNotMatch(demoPerformance, /class="demo-widget-title"/);
  assert.equal((demoPerformance.match(/<p>/g) ?? []).length, 4);
  assert.match(demoPerformance, /GAME CPU/);
  assert.match(demoPerformance, /GAME RAM/);
  assert.match(demoPerformance, /CPU TEMP/);
  assert.match(demoPerformance, /GPU TEMP/);
});

test("mission headings reserve the checkbox column used by mission rows", () => {
  assert.equal((html.match(/class="mission-title"><i aria-hidden="true"><\/i>/g) ?? []).length, 2);
  assert.match(css, /\.mission-title > i\s*{[^}]*visibility:\s*hidden;/s);
});

test("the footer carries the ValhallaB ownership notice", () => {
  assert.match(
    html,
    /© <span data-current-year>2026<\/span> VALHALLAB\. ALL RIGHTS RESERVED\./,
  );
  assert.doesNotMatch(html, /© <span data-current-year>2026<\/span> PlayerVox/);
});
