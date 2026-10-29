import { open, test, expect } from "@nlayer/test";

test("the system scheme is read and a disabled view blocks a press", () => {
  const app = open("App.tsx", { width: 390, height: 844, colorScheme: "dark" });
  app.waitFor("text:scheme:dark presses:0");
  app.waitFor("role:button[inner]");
  expect(() => app.press("label:inner")).toThrow();
  app.waitFor("text:scheme:dark presses:0");
  app.close();
});
