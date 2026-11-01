import { expect, open, test, type Node } from "@nlayer/test";

function find(node: Node, name: string): Node | null {
  if (node.testID === name) return node;
  for (const child of node.children ?? []) {
    const found = find(child, name);
    if (found) return found;
  }
  return null;
}

function mustFind(node: Node, name: string): Node {
  const found = find(node, name);
  if (found === null) throw new Error(`\`${name}\` is not in the tree`);
  return found;
}

// A pan drives the view's transform (its layout frame does not move); the
// tree reports the live `translateX` step, as animated values last moved it.
function translateX(node: Node): number {
  for (const step of node.transform ?? []) {
    if (step.translateX !== undefined) return step.translateX;
  }
  return 0;
}

test("pan drives animated value and springs back on release", () => {
  const app = open("App.tsx");

  // Initial position
  const initial = mustFind(app.tree(), "panCard");
  expect(translateX(initial)).toBe(0);

  // Drag horizontally and hold, so the value is read before the spring runs.
  app.dragAndHold("panCard", 120, 0);
  app.advance(50);

  // Should have moved right
  const dragged = mustFind(app.tree(), "panCard");
  expect(translateX(dragged) > 100).toBe(true);

  // After release, spring back to 0
  app.drop();
  app.advance(500);
  const sprung = mustFind(app.tree(), "panCard");
  expect(Math.abs(translateX(sprung)) < 5).toBe(true);

  app.close();
});

test("horizontal pan in vertical scroll does not trigger scroll", () => {
  const app = open("App.tsx");

  // Record initial scroll position
  const initialY = mustFind(app.tree(), "scroll").offset?.y ?? 0;

  // Drag card horizontally inside scroll
  app.drag("scrollPanCard", 50, 0);
  app.advance(50);

  // Scroll position should not change
  const afterY = mustFind(app.tree(), "scroll").offset?.y ?? 0;
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
