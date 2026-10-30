import { expect, open, test } from "@nlayer/test";

test("the page animation finishes", () => {
  const app = open("App.tsx");
  app.waitFor("done");
  expect(app.texts()).toContain("done");
  app.close();
});
