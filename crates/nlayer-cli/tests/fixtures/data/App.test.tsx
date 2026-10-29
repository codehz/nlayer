import { expect, open, test } from "@nlayer/test";

test("storage survives a restart", () => {
  const first = open("./App.tsx", { dataDir: "./data" });
  first.waitFor("Reset");
  // A device keeps its entries between runs: start from no draft.
  first.press("id:reset");
  first.waitFor("none");
  first.press("id:save");
  first.waitFor("kept");
  first.close();

  const second = open("./App.tsx", { dataDir: "./data" });
  second.waitFor("kept");
  expect(second.texts()).toContain("kept");
  second.close();
});

test("a write past the quota rejects with the code", () => {
  const app = open("./App.tsx", { dataDir: "./data" });
  app.waitFor("Full");
  app.press("id:full");
  app.waitFor("QuotaExceededError ERR_NLAYER_QUOTA_EXCEEDED");
  app.close();
});
