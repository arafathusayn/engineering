// The runtime globals templates/app.js publishes on the page, so the
// browser-side callbacks in the tests type-check.
export {};

declare global {
  interface Window {
    __canon: { edges: [string, string][]; misses: string[] };
  }
}
