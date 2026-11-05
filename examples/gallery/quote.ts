declare global {
  var __nlayerTestOrigin: string | undefined;
}

/** Headless tests serve the app directory here. The browser fetches the same file from the page. */
export function quoteUrl(): string {
  const origin = globalThis.__nlayerTestOrigin;
  if (typeof origin === "string" && origin !== "") return `${origin}/quote.json`;
  return "/quote.json";
}
