// End-to-end tests for templates/app.js against the real built page.
// Coverage of app.js is recorded for every page and asserted 100% by the
// final test. See server.mjs for the page variants used to reach defensive
// branches the real content never triggers.
import { test, expect } from "./fixtures.mjs";

// Content facts are derived from the served page, not hard-coded, so the
// suite keeps testing app.js (not canon.toon) as content evolves.
let TOTAL, FULL_COUNT, EDGES, CAT, ISOLATED_ID;
test.beforeAll(async ({ server }) => {
  const html = await (await fetch(server + "/")).text();
  TOTAL = (html.match(/<article/g) || []).length;
  // The minifier reorders/unquotes attributes; parse chip tags leniently.
  const cats = [...html.matchAll(/<button[^>]*class=chip[^>]*>/g)]
    .map((m) => m[0].match(/data-cat=(?:"([^"]*)"|([^ >]+))/))
    .map((dm) => dm[1] ?? dm[2])
    .filter((c) => c !== "All");
  FULL_COUNT = `${TOTAL} entries · ${cats.length} categories`;
  const blob = html.match(/id=edgeData[^>]*>(.*?)<\/script>/s) ?? html.match(/id="edgeData"[^>]*>(.*?)<\/script>/s);
  EDGES = JSON.parse(blob[1]).edges;
  // a chip that filters to a strict subset
  CAT = cats.find((c) => c !== "All");
  // a chrono row connected to no lineage edge at all
  const connected = new Set(EDGES.flatMap((e) => [e[0], e[1]]));
  ISOLATED_ID = [...html.matchAll(/class=crow data-id=([a-z0-9-]+)/g)]
    .map((m) => m[1])
    .find((id) => !connected.has(id));
});

const visibleCards = (page) =>
  page.evaluate(() => [...document.querySelectorAll("#main article.card")].filter((a) => !a.hidden).length);
const countLine = (page) => page.locator("#countLine").textContent();

test.describe("boot", () => {
  test("exposes the lineage graph and derives state from the DOM", async ({ page, server }) => {
    await page.goto(server + "/");
    const canon = await page.evaluate(() => window.__canon);
    expect(canon.misses).toEqual([]);
    // __canon re-exposes the embedded blob's pairs, in order
    expect(canon.edges).toEqual(EDGES.map((e) => [e[0], e[1]]));
    await expect(page.locator("#main article.card")).toHaveCount(TOTAL);
    expect(await countLine(page)).toBe(FULL_COUNT);
    // The anchor namespace is read from the page, not hard-coded.
    expect(await page.evaluate(() => document.getElementById("main").dataset.cardPrefix)).toBe("card-");
  });

  test("reconciles search text that predates the script (restored form value)", async ({ page, server }) => {
    await page.goto(server + "/?prefill=singleton");
    expect(await visibleCards(page)).toBeLessThan(TOTAL);
    expect(await countLine(page)).toMatch(new RegExp(`^\\d+ of ${TOTAL} entries$`));
  });
});

test.describe("search", () => {
  test("filters cards, hides intros and empty sections, restores on clear", async ({ page, server }) => {
    await page.goto(server + "/");
    await page.fill("#q", "solid");
    const n = await visibleCards(page);
    expect(n).toBeGreaterThan(0);
    expect(n).toBeLessThan(TOTAL);
    expect(await countLine(page)).toBe(`${n} of ${TOTAL} entries`);
    expect(await page.locator("#main .intro:visible").count()).toBe(0);
    expect(await page.locator("#main .section:visible h2").count()).toBeGreaterThan(0);
    await page.fill("#q", "");
    expect(await visibleCards(page)).toBe(TOTAL);
    expect(await countLine(page)).toBe(FULL_COUNT);
    expect(await page.locator("#main .intro:visible").count()).toBeGreaterThan(1);
  });

  test("whitespace-only queries match everything; garbage shows the empty state", async ({ page, server }) => {
    await page.goto(server + "/");
    await page.fill("#q", "   ");
    expect(await visibleCards(page)).toBe(TOTAL);
    await page.fill("#q", "zzz-no-such-term");
    expect(await visibleCards(page)).toBe(0);
    await expect(page.locator("#empty")).toBeVisible();
    await page.fill("#q", "");
    await expect(page.locator("#empty")).toBeHidden();
  });

  test("multi-token queries AND together; case-insensitive", async ({ page, server }) => {
    await page.goto(server + "/");
    await page.fill("#q", "SOLID liskov");
    const n = await visibleCards(page);
    expect(n).toBeGreaterThan(0);
    await page.fill("#q", "solid");
    expect(await visibleCards(page)).toBeGreaterThanOrEqual(n);
  });

  test("link labels are not searchable (haystack excludes sources)", async ({ page, server }) => {
    await page.goto(server + "/");
    await page.fill("#q", "wikipedia");
    expect(await visibleCards(page)).toBe(0);
  });

  test("/ focuses the box (but types normally inside it); Escape clears and blurs", async ({ page, server }) => {
    await page.goto(server + "/");
    await page.keyboard.press("/");
    await expect(page.locator("#q")).toBeFocused();
    await page.keyboard.press("/"); // now goes INTO the box
    await expect(page.locator("#q")).toHaveValue("/");
    await page.keyboard.press("Escape");
    await expect(page.locator("#q")).toHaveValue("");
    await expect(page.locator("#q")).not.toBeFocused();
    await page.keyboard.press("Escape"); // Escape with nothing focused/selected: no-op
    expect(await visibleCards(page)).toBe(TOTAL);
  });
});

test.describe("category chips", () => {
  test("filter to one category and back to All", async ({ page, server }) => {
    await page.goto(server + "/");
    const testing = page.locator(`#chips .chip[data-cat="${CAT}"]`);
    await testing.click();
    await expect(testing).toHaveAttribute("aria-pressed", "true");
    const n = await visibleCards(page);
    expect(n).toBeGreaterThan(0);
    expect(n).toBeLessThan(TOTAL);
    expect(await page.locator("#main .section:visible").count()).toBe(1);
    // intros stay visible under a pure category filter (no search text)
    expect(await page.locator("#main .intro:visible").count()).toBe(1);
    await page.locator('#chips .chip[data-cat="All"]').click();
    expect(await visibleCards(page)).toBe(TOTAL);
  });

  test("search and category compose; clicks on the bar itself are ignored", async ({ page, server }) => {
    await page.goto(server + "/");
    await page.locator(`#chips .chip[data-cat="${CAT}"]`).click();
    await page.fill("#q", "zzz-no-such-term");
    expect(await visibleCards(page)).toBe(0);
    await page.evaluate(() => {
      document.getElementById("chips").dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(await visibleCards(page)).toBe(0); // nothing changed
  });
});

test.describe("view toggle and chrono", () => {
  test("chrono shows decade-grouped rows and lineage curves; list hides", async ({ page, server }) => {
    await page.goto(server + "/");
    await page.click('#viewSeg button[data-v="chrono"]');
    await expect(page.locator("#listView")).toBeHidden();
    await expect(page.locator("#chronoView")).toBeVisible();
    await expect(page.locator('#viewSeg button[data-v="chrono"]')).toHaveAttribute("aria-pressed", "true");
    await page.waitForTimeout(100); // rAF draw
    expect(await page.locator(".chrono .crow:visible").count()).toBe(TOTAL);
    expect(await page.locator("svg.cedges path.ce").count()).toBe(EDGES.length);
    expect(await page.locator("svg.cedges path.ce.x").count()).toBeGreaterThan(0);
    expect(await countLine(page)).toBe(`${TOTAL} of ${TOTAL} entries · ${EDGES.length} lineage links in view`);
    // clicks on the segmented control's padding are ignored
    await page.evaluate(() => {
      document.getElementById("viewSeg").dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    await expect(page.locator("#chronoView")).toBeVisible();
    await page.click('#viewSeg button[data-v="list"]');
    await expect(page.locator("#listView")).toBeVisible();
    await expect(page.locator("#chronoView")).toBeHidden();
  });

  test("filtering in chrono hides rows, decade headers, and shrinks the canvas", async ({ page, server }) => {
    await page.goto(server + "/");
    await page.click('#viewSeg button[data-v="chrono"]');
    await page.waitForTimeout(100);
    const fullH = await page.evaluate(() => document.querySelector(".chrono").scrollHeight);
    const fullDecades = await page.locator(".chrono .cdecade:visible").count();
    await page.fill("#q", "solid liskov");
    await page.waitForTimeout(100);
    expect(await page.locator(".chrono .crow:visible").count()).toBeGreaterThan(0);
    expect(await page.locator(".chrono .cdecade:visible").count()).toBeLessThan(fullDecades);
    const filteredH = await page.evaluate(() => document.querySelector(".chrono").scrollHeight);
    expect(filteredH).toBeLessThan(fullH / 3); // no svg ratchet
    expect(await countLine(page)).toMatch(/lineage links in view$/);
    // garbage query: empty chrono
    await page.fill("#q", "zzz-no-such-term");
    await page.waitForTimeout(100);
    await expect(page.locator("#empty")).toBeVisible();
    expect(await page.locator(".chrono .crow:visible").count()).toBe(0);
    await page.fill("#q", "");
    await page.waitForTimeout(100);
    expect(await page.evaluate(() => document.querySelector(".chrono").scrollHeight)).toBeGreaterThan(fullH * 0.9);
  });

  test("rapid chrono→list flip cancels the pending frame; resize redraws only chrono", async ({ page, server }) => {
    await page.goto(server + "/");
    await page.evaluate(() => {
      document.querySelector('#viewSeg button[data-v="chrono"]').click();
      document.querySelector('#viewSeg button[data-v="list"]').click();
    });
    await page.waitForTimeout(150);
    expect(await countLine(page)).toBe(FULL_COUNT); // list text, not stale chrono text
    // resize in list view: handler returns immediately
    await page.setViewportSize({ width: 900, height: 700 });
    await page.waitForTimeout(200);
    // chrono + narrow viewport: narrow lane geometry, debounced redraw
    await page.click('#viewSeg button[data-v="chrono"]');
    await page.waitForTimeout(100);
    await page.setViewportSize({ width: 500, height: 700 });
    await page.setViewportSize({ width: 520, height: 700 }); // second resize clears the first timer
    await page.waitForTimeout(250);
    expect(await page.locator("svg.cedges path.ce").count()).toBe(EDGES.length);
    // resize in chrono then flip to list before the debounce fires
    await page.setViewportSize({ width: 800, height: 700 });
    await page.click('#viewSeg button[data-v="list"]');
    await page.waitForTimeout(250); // debounced callback sees view=list and skips
    expect(await countLine(page)).toBe(FULL_COUNT);
  });

  test("chain highlighting: select, toggle off, keyboard, Escape, isolated rows", async ({ page, server }) => {
    await page.goto(server + "/");
    await page.click('#viewSeg button[data-v="chrono"]');
    await page.waitForTimeout(100);
    const richId = EDGES.find((e) => EDGES.some((f) => f[0] === e[1]))[1]; // a node with both parents and children
    const tdd = page.locator(`.crow[data-id="${richId}"]`);
    await tdd.click();
    await expect(tdd).toHaveClass(/sel/);
    await expect(page.locator(".chrono.dimmed")).toHaveCount(1);
    expect(await page.locator(".crow.lit").count()).toBeGreaterThan(2);
    expect(await page.locator("path.ce.lit").count()).toBeGreaterThan(0);
    await tdd.click(); // toggle off
    await expect(page.locator(".chrono.dimmed")).toHaveCount(0);
    // keyboard activation
    await tdd.press("Enter");
    await expect(tdd).toHaveClass(/sel/);
    await tdd.press(" ");
    await expect(tdd).not.toHaveClass(/sel/);
    // an undated, unconnected row lights only itself
    expect(ISOLATED_ID, "the canon has an unconnected card").toBeTruthy();
    const lone = page.locator(`.crow[data-id="${ISOLATED_ID}"]`);
    await lone.click();
    expect(await page.locator(".crow.lit").count()).toBe(1);
    await page.keyboard.press("Escape");
    await expect(page.locator(".chrono.dimmed")).toHaveCount(0);
    // Enter on a non-row target inside #main is left alone
    await page.locator(".crow a.go").first().press("Enter");
    await expect(page.locator("#listView")).toBeVisible(); // the link navigated to list view
  });

  test("dense synthetic graph exercises deep lane assignment", async ({ page, server }) => {
    await page.goto(server + "/?dense=1");
    await page.click('#viewSeg button[data-v="chrono"]');
    await page.waitForTimeout(100);
    expect(await page.locator("svg.cedges path.ce").count()).toBe(16);
    expect(await page.evaluate(() => window.__canon.edges.length)).toBe(16);
  });
});

test.describe("cross-reference navigation", () => {
  test("xref click navigates, pulses, and resets filters only when needed", async ({ page, server }) => {
    await page.goto(server + "/");
    // Discover one cross-category and one same-category xref from the DOM.
    const found = await page.evaluate(() => {
      const result = {};
      for (const a of document.querySelectorAll("#main .lineage a.xref")) {
        const from = a.closest("article.card").dataset.cat;
        const to = document.getElementById("card-" + a.dataset.id)?.dataset.cat;
        if (!to) continue;
        if (!result.cross && from !== to) result.cross = { from, to, id: a.dataset.id };
        if (!result.local && from === to) result.local = { from, id: a.dataset.id };
        if (result.cross && result.local) break;
      }
      return result;
    });
    expect(found.cross, "the canon has cross-category lineage").toBeTruthy();
    expect(found.local, "the canon has same-category lineage").toBeTruthy();

    // active category ≠ target's category → falls back to All
    await page.locator(`#chips .chip[data-cat="${found.cross.from}"]`).click();
    await page
      .locator(`#main .section[data-cat="${found.cross.from}"] a.xref[data-id="${found.cross.id}"]`)
      .first()
      .click();
    await expect(page.locator("#card-" + found.cross.id)).toHaveClass(/pulse/);
    await expect(page.locator('#chips .chip[data-cat="All"]')).toHaveAttribute("aria-pressed", "true");

    // active category = target's category → the filter is kept
    await page.locator(`#chips .chip[data-cat="${found.local.from}"]`).click();
    await page
      .locator(`#main .section[data-cat="${found.local.from}"] a.xref[data-id="${found.local.id}"]`)
      .first()
      .click();
    await expect(page.locator(`#chips .chip[data-cat="${found.local.from}"]`)).toHaveAttribute(
      "aria-pressed",
      "true"
    );
  });

  test("navigating from chrono switches views and clears search first", async ({ page, server }) => {
    await page.goto(server + "/");
    await page.fill("#q", "test");
    await page.click('#viewSeg button[data-v="chrono"]');
    await page.waitForTimeout(100);
    const go = page.locator(".crow:visible a.go").first();
    const id = await go.getAttribute("data-id");
    await go.click();
    await expect(page.locator("#listView")).toBeVisible();
    await expect(page.locator("#q")).toHaveValue("");
    await expect(page.locator("#card-" + id)).toHaveClass(/pulse/);
  });

  test("pulse: retrigger moves it, re-render clears it, and it expires", async ({ page, server }) => {
    await page.goto(server + "/");
    const refs = page.locator("#main a.xref:visible");
    await refs.nth(0).click();
    const first = await refs.nth(0).getAttribute("data-id");
    await refs.nth(1).click();
    const second = await refs.nth(1).getAttribute("data-id");
    if (first !== second) {
      await expect(page.locator("#card-" + first)).not.toHaveClass(/pulse/);
    }
    await expect(page.locator("#card-" + second)).toHaveClass(/pulse/);
    // any re-render clears it immediately (parity with the old rebuild)
    await page.fill("#q", "x");
    await expect(page.locator(".card.pulse")).toHaveCount(0);
    await page.fill("#q", "");
    // natural expiry
    await refs.nth(0).click();
    await expect(page.locator(".card.pulse")).toHaveCount(1);
    await page.waitForTimeout(1900);
    await expect(page.locator(".card.pulse")).toHaveCount(0);
  });

  test("unknown or missing targets are handled gracefully", async ({ page, server }) => {
    await page.goto(server + "/");
    await page.evaluate(() => {
      const a = document.createElement("a");
      a.className = "xref";
      a.dataset.id = "no-such-card";
      a.href = "#card-no-such-card";
      a.textContent = "ghost";
      document.querySelector("#main .lineage").appendChild(a);
    });
    await page.click('a.xref[data-id="no-such-card"]');
    expect(await visibleCards(page)).toBe(TOTAL); // no-op, no crash
    // target known to the index but removed from the DOM afterwards
    const real = page.locator("#main a.xref:visible").first();
    const id = await real.getAttribute("data-id");
    await page.evaluate((x) => document.getElementById("card-" + x).remove(), id);
    await real.click();
    await expect(page.locator(".card.pulse")).toHaveCount(0); // scrolled nowhere, no crash
    // clicks on plain section furniture do nothing
    await page.locator("#main h2").first().click();
  });
});

test.describe("appearance", () => {
  test("menu toggling, selection, and outside-pointer close", async ({ page, server }) => {
    await page.goto(server + "/");
    await page.click("#themeBtn");
    await expect(page.locator("#themeMenu")).toBeVisible();
    await page.click("#themeBtn"); // toggles closed
    await expect(page.locator("#themeMenu")).toBeHidden();
    await page.click("#themeBtn");
    await page.click('#themeMenu button[data-t="dark"]');
    expect(await page.evaluate(() => document.documentElement.dataset.theme)).toBe("dark");
    expect(await page.evaluate(() => document.getElementById("metaTheme").content)).toBe("#000000");
    await page.click("#themeBtn");
    await page.click('#themeMenu button[data-t="light"]');
    expect(await page.evaluate(() => document.documentElement.dataset.theme)).toBe("light");
    expect(await page.evaluate(() => document.getElementById("metaTheme").content)).toBe("#f5f5f7");
    await page.click("#themeBtn");
    await page.click('#themeMenu button[data-t="auto"]');
    expect(await page.evaluate(() => "theme" in document.documentElement.dataset)).toBe(false);
    // pointer inside the appearance widget keeps the menu open; outside closes
    await page.click("#themeBtn");
    await page.locator("#themeMenu").dispatchEvent("pointerdown", { bubbles: true });
    await expect(page.locator("#themeMenu")).toBeVisible();
    await page.locator(".hero h1").dispatchEvent("pointerdown", { bubbles: true });
    await expect(page.locator("#themeMenu")).toBeHidden();
    await page.locator(".hero h1").dispatchEvent("pointerdown", { bubbles: true }); // menu already hidden
  });

  test("full keyboard model", async ({ page, server }) => {
    await page.goto(server + "/");
    await page.focus("#themeBtn");
    await page.keyboard.press("ArrowDown"); // opens, focuses checked item
    await expect(page.locator("#themeMenu")).toBeVisible();
    await expect(page.locator('#themeMenu button[data-t="auto"]')).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(page.locator('#themeMenu button[data-t="light"]')).toBeFocused();
    await page.keyboard.press("ArrowUp");
    await page.keyboard.press("End");
    await expect(page.locator('#themeMenu button[data-t="dark"]')).toBeFocused();
    await page.keyboard.press("Home");
    await expect(page.locator('#themeMenu button[data-t="auto"]')).toBeFocused();
    await page.keyboard.press("x"); // unhandled key falls through
    await page.keyboard.press("Escape");
    await expect(page.locator("#themeMenu")).toBeHidden();
    await expect(page.locator("#themeBtn")).toBeFocused();
    // ArrowDown on the button while the menu is already open is left alone
    await page.keyboard.press("ArrowDown");
    await page.evaluate(() => {
      document.getElementById("themeBtn").dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true })
      );
    });
    await expect(page.locator("#themeMenu")).toBeVisible();
    // Tab out closes without refocusing the button
    await page.keyboard.press("Tab");
    await expect(page.locator("#themeMenu")).toBeHidden();
  });

  test("auto mode follows the OS scheme; explicit modes do not", async ({ page, server }) => {
    await page.goto(server + "/");
    const meta = () => page.evaluate(() => document.getElementById("metaTheme").content);
    // media change events propagate asynchronously in Chromium
    await page.emulateMedia({ colorScheme: "dark" });
    await expect.poll(meta).toBe("#000000");
    await page.emulateMedia({ colorScheme: "light" });
    await expect.poll(meta).toBe("#f5f5f7");
    await page.click("#themeBtn");
    await page.click('#themeMenu button[data-t="dark"]');
    await page.emulateMedia({ colorScheme: "dark" }); // listener guard: not auto
    await page.waitForTimeout(300);
    expect(await meta()).toBe("#000000");
  });
});

test.describe("reduced motion", () => {
  test.use({ contextOptions: { reducedMotion: "reduce" } });
  test("category jumps and card navigation use instant scrolling", async ({ page, server }) => {
    await page.goto(server + "/");
    await page.locator(`#chips .chip[data-cat="${CAT}"]`).click();
    expect(await visibleCards(page)).toBeLessThan(TOTAL);
    await page.locator("#main a.xref:visible").first().click();
    await expect(page.locator(".card.pulse")).toHaveCount(1);
  });
});

test.describe("degraded content (defensive branches)", () => {
  test("rows without labels, grids without subheads, sections without intros", async ({ page, server }) => {
    await page.goto(server + "/?edgecases=1");
    await expect(page.locator("#main article.card")).toHaveCount(TOTAL);
    // the label-less use-row contributes an empty haystack slot but still filters
    await page.fill("#q", "solid");
    expect(await visibleCards(page)).toBeGreaterThan(0);
    await page.fill("#q", "");
    expect(await visibleCards(page)).toBe(TOTAL);
  });
});
