import { expect, type Locator } from '@playwright/test';

/** Check every clipping ancestor, allowing only browser subpixel intersection rounding. */
export async function expectOverviewContained(locator: Locator) {
  await expect(locator).toBeVisible();
  await expect
    .poll(() =>
      locator.evaluate(
        (node) =>
          new Promise<number>((resolve) => {
            const observer = new IntersectionObserver(([entry]) => {
              observer.disconnect();
              const bounds = entry.boundingClientRect;
              const visible = entry.intersectionRect;
              resolve(
                Math.max(
                  visible.left - bounds.left,
                  visible.top - bounds.top,
                  bounds.right - visible.right,
                  bounds.bottom - visible.bottom,
                ),
              );
            });
            observer.observe(node);
          }),
      ),
    )
    .toBeLessThanOrEqual(1 / 4);
}
