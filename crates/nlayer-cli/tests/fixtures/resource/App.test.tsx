import { expect, open, test } from "@nlayer/test";

test("a resource loads, fails keeping the value, and refetches", () => {
  const app = open("./App.tsx");
  app.waitFor("unresolved - aborts 0");
  app.press("id:load");
  app.waitFor("ready hello 1 aborts 0");
  app.press("id:fail");
  app.waitFor("errored hello 1 aborts 0");
  app.press("id:again");
  app.waitFor("ready hello 2 aborts 0");
  expect(app.texts()).toContain("ready hello 2 aborts 0");
  app.close();
});
