import { expect, open, test } from "@nlayer/test";

test("the page animation finishes", () => {
  const app = open("App.tsx");
  app.waitFor("done stopped");
  expect(app.texts().join(" ")).toContain("done");
  app.close();
});
