// A category-chip click must scroll the live entry count (#countLine) into view
// just below the sticky toolbar — not jump to the top of the hero, which was the
// old behavior (window.scrollTo({top:0})). After a filter, the updated tally and
// the results beneath it are what the reader wants to see, not the masthead.
//
// The scroll offset is the toolbar's *measured* height because the toolbar wraps
// taller on narrow viewports (~111px at 1200w, ~145px at 390w); a fixed offset
// would leave the line hidden behind the chrome on mobile.
//
// This file sorts before coverage.spec.ts, so its page loads feed the
// 100%-coverage and no-console-error gates asserted there.
import { test, expect } from "./fixtures";

// Instant (non-animated) scrolling so the settled position reads deterministically.
test.use({ contextOptions: { reducedMotion: "reduce" } });

const WIDTHS = [1200, 390];

test.describe("chip click reveals the entry count", () => {
  for (const width of WIDTHS) {
    test(`clicking a category chip scrolls #countLine below the toolbar (w=${width})`, async ({
      page,
      server,
    }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(server + "/");

      // Start deep in the page (hero offscreen) — the state where the old
      // jump-to-hero-top was most jarring.
      await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));

      // Click a real filter (not the already-active "All").
      await page.locator("#chips .chip[data-cat]:not([data-cat='All'])").first().click();

      const geo = await page.evaluate(() => {
        const c = document.getElementById("countLine")!.getBoundingClientRect();
        const t = document.querySelector<HTMLElement>(".toolbar")!.getBoundingClientRect();
        const maxScroll = document.documentElement.scrollHeight - window.innerHeight;
        return {
          scrollY: window.scrollY,
          maxScroll,
          countTop: c.top,
          countBottom: c.bottom,
          toolbarBottom: t.bottom,
          innerHeight: window.innerHeight,
        };
      });

      // Regression: the old handler jumped to the hero top (scrollY === 0).
      expect(geo.scrollY, "the page scrolled away from the top").toBeGreaterThan(0);

      // #countLine clears the sticky toolbar (never hidden behind it) and is
      // fully in view.
      expect(
        geo.countTop,
        "#countLine is not hidden under the toolbar",
      ).toBeGreaterThanOrEqual(geo.toolbarBottom - 1);
      expect(geo.countBottom, "#countLine is fully visible").toBeLessThanOrEqual(
        geo.innerHeight + 1,
      );

      // It lands snug just under the toolbar — unless the filtered page is too
      // short to scroll that far, in which case it is pinned at max scroll.
      const snug = Math.abs(geo.countTop - geo.toolbarBottom) <= 2;
      const atMax = geo.scrollY >= geo.maxScroll - 1;
      expect(snug || atMax, "#countLine sits just below the toolbar").toBe(true);
    });
  }
});
