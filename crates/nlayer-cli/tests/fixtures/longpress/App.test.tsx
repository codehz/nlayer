import { expect, open, test } from "@nlayer/test";

test("a long press does not also press", () => {
  const app = open("App.tsx");
  app.longPress("id:target");
  app.waitFor("long:1 short:0");
  expect(app.texts()).toContain("long:1 short:0");
  app.close();
});
