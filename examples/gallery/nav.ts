import { signal } from "@nlayer/runtime";
import { createNavStack } from "@nlayer/ui";

export type Route =
  | { name: "home" }
  | { name: "view" }
  | { name: "pointer" }
  | { name: "pointer-events" }
  | { name: "image" }
  | { name: "scroll" }
  | { name: "input" }
  | { name: "fetch" }
  | { name: "platform" }
  | { name: "storage" }
  | { name: "animation" }
  | { name: "virtual-list" }
  | { name: "detail"; id: number }
  | { name: "dialog" }
  | { name: "gesture" }
  | { name: "list" }
  | { name: "header" }
  | { name: "viewport-parent" }
  | { name: "reading" }
  | { name: "classes" }
  | { name: "effects" };

export const nav = createNavStack<Route>([{ name: "home" }]);
export const detailInterceptBack = signal(false);
export const detailBackIntercepted = signal(false);
export const dismissedCount = signal(0);
/** Header demo: whether `standardAppearance` has no opaque background. */
export const headerTransparent = signal(false);
/** Header demo: the static large title (`largeTitleDisplayMode="always"`). */
export const headerLargeTitle = signal(false);
/** Header demo: leading items next to the system back instead of replacing it. */
export const headerSupplement = signal(false);

/** Counts a screen the user popped. Every screen hands this to its `NavPage`. */
export function dismiss(): void {
  dismissedCount.set(dismissedCount() + 1);
}
