import { signal } from "@nlayer/runtime";
import { createNavStack } from "@nlayer/ui";

export type Route =
  | { name: "home" }
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
