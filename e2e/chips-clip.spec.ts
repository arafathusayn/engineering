// Layout regression: the horizontally-scrolling filter pills (#chips) must be
// clipped on the same vertical line as the appearance (dark/light) icon in the
// toolbar. Before the fix a partially-scrolled row was cut at the raw viewport
// edge, ~1rem to the right of the icon; only the fully-scrolled end lined up.
//
// This file sorts before coverage.spec.ts, so its page loads still feed the
// 100%-coverage and no-console-error gates asserted there.
import { test, expect } from "./fixtures";

// Widths where the appearance icon is the right-most toolbar item, matching the
// reported scenario (icon at the top-right, pills clipped below it): a desktop
// width and a phone width (there the search box wraps to its own row).
const WIDTHS = [1200, 390];

test.describe("chips clip alignment", () => {
  for (const width of WIDTHS) {
    test(`the pill row is clipped on the appearance icon's right edge (w=${width})`, async ({
      page,
      server,
    }) => {
      await page.setViewportSize({ width, height: 800 });
      await page.goto(server + "/");

      const geo = await page.evaluate(() => {
        const chips = document.getElementById("chips")!;
        const icon = document.getElementById("themeBtn")!;

        // The reported bug state: scrolled, but NOT to the right-most end.
        const maxScroll = chips.scrollWidth - chips.clientWidth;
        chips.scrollLeft = Math.floor(maxScroll / 2);

        // Where the row is visually clipped: right edge of the clip-path inset.
        const clip = getComputedStyle(chips).clipPath; // e.g. "inset(0px 24px 0px 0px)"
        const nums = clip.match(/inset\(([^)]*)\)/)?.[1].trim().split(/\s+/).map(parseFloat) ?? [];
        // inset(top right bottom left); fewer values mirror per the shorthand.
        const rightInset = nums.length >= 2 ? nums[1] : nums[0];
        const chipsRect = chips.getBoundingClientRect();
        const clipRight = chipsRect.right - rightInset;
        const iconRight = icon.getBoundingClientRect().right;

        // Proof the clip actually cuts content here: some pill reaches past it.
        const pills = [...chips.querySelectorAll<HTMLElement>(".chip")];
        const overflowsClip = pills.some((c) => c.getBoundingClientRect().right > clipRight + 1);

        // The already-correct state must stay correct: scrolled fully right, the
        // last pill is flush with that same line.
        chips.scrollLeft = chips.scrollWidth;
        const lastPillRight = pills[pills.length - 1].getBoundingClientRect().right;

        return {
          hasClip: clip !== "none",
          maxScroll,
          rightInset,
          clipRight,
          iconRight,
          overflowsClip,
          lastPillRight,
        };
      });

      // The row genuinely overflows (otherwise the test proves nothing).
      expect(geo.maxScroll, "the pill row scrolls horizontally").toBeGreaterThan(0);
      expect(geo.hasClip, "#chips defines a right-edge clip").toBe(true);
      expect(geo.overflowsClip, "a pill is being clipped at this scroll position").toBe(true);

      // The clip lands on the appearance icon's right edge (sub-pixel tolerance).
      expect(Math.abs(geo.clipRight - geo.iconRight)).toBeLessThanOrEqual(1);

      // Scrolled to the end, the last pill is flush with that same line.
      expect(Math.abs(geo.lastPillRight - geo.iconRight)).toBeLessThanOrEqual(1.5);
    });
  }
});
