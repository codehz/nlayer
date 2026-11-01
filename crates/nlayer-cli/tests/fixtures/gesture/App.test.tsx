import { expect, open, test } from "@nlayer/test";

function find(node: any, name: string): any {
  if (node.testID === name) return node;
  for (const child of node.children ?? []) {
    const found = find(child, name);
    if (found) return found;
  }
  return null;
}

test("pan drives animated value and springs back on release", () => {
  const app = open("App.tsx");
  
  // Initial position
  const initial = find(app.tree(), "panCard");
  expect(initial.frame.x).toBe(0);
  
  // Drag horizontally
  app.drag("panCard", 120, 0);
  app.advance(50);
  
  // Should have moved right
  const dragged = find(app.tree(), "panCard");
  expect(dragged.frame.x > 100).toBe(true);
  
  // After release, spring back to 0
  app.advance(500);
  const sprung = find(app.tree(), "panCard");
  expect(Math.abs(sprung.frame.x) < 5).toBe(true);
  
  app.close();
});

test("horizontal pan in vertical scroll does not trigger scroll", () => {
  const app = open("App.tsx");
  
  // Record initial scroll position
  const initialScroll = find(app.tree(), "scroll");
  const initialY = initialScroll.scrollY || 0;
  
  // Drag card horizontally inside scroll
  app.drag("scrollPanCard", 50, 0);
  app.advance(50);
  
  // Scroll position should not change
  const afterDrag = find(app.tree(), "scroll");
  const afterY = afterDrag.scrollY || 0;
  expect(afterY).toBe(initialY);
  
  app.close();
});

// Pinch/rotation: bindings exist and don't crash, but headless doesn't drive them.
// Per docs/v3.md: "pinch/rotation have no headless steps (nsh also has none),
// only smoke on --target". Real platform tests would use multi-touch input
// which is not available in automated web/iOS/Android testing without platform-
// specific multi-touch injection APIs. This smoke just ensures bindings are present.
test("pinch and rotation bindings present without errors", () => {
  const app = open("App.tsx");
  
  // Verify views with bindings exist (toBeTruthy not available, check not null)
  const pinch = find(app.tree(), "pinchCard");
  const rotation = find(app.tree(), "rotationCard");
  expect(pinch !== null).toBe(true);
  expect(rotation !== null).toBe(true);
  
  app.close();
});
