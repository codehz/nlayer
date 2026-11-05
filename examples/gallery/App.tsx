import { mount } from "@nlayer/runtime";
import { NavStack } from "@nlayer/ui";
import { nav, type Route } from "./nav";
import { AnimationPage } from "./screens/animation";
import { ClassesPage } from "./screens/classes";
import { DetailPage } from "./screens/detail";
import { DialogDemoPage } from "./screens/dialog";
import { EffectsPage } from "./screens/effects";
import { FetchPage } from "./screens/fetch";
import { GestureCardPage } from "./screens/gesture";
import { HeaderDemoPage } from "./screens/header";
import { HomePage } from "./screens/home";
import { ImagePage } from "./screens/image";
import { InputPage } from "./screens/input";
import { ListDemoPage } from "./screens/list";
import { PointerPage } from "./screens/pointer";
import { PlatformPage } from "./screens/platform";
import { ReadingPage } from "./screens/reading";
import { ScrollPage } from "./screens/scroll";
import { StoragePage } from "./screens/storage";
import { TextPage } from "./screens/text";
import { ViewPage } from "./screens/layout";
import { ViewportParentPage } from "./screens/viewport";
import { VirtualListPage } from "./screens/virtual-list";

/** Each screen owns its `<NavPage>`; the stack only picks the component. */
const screens: Record<Route["name"], () => unknown> = {
  home: HomePage,
  view: ViewPage,
  pointer: PointerPage,
  text: TextPage,
  image: ImagePage,
  scroll: ScrollPage,
  input: InputPage,
  fetch: FetchPage,
  platform: PlatformPage,
  storage: StoragePage,
  animation: AnimationPage,
  "virtual-list": VirtualListPage,
  detail: DetailPage,
  dialog: DialogDemoPage,
  gesture: GestureCardPage,
  list: ListDemoPage,
  header: HeaderDemoPage,
  "viewport-parent": ViewportParentPage,
  reading: ReadingPage,
  classes: ClassesPage,
  effects: EffectsPage,
};

function App() {
  return (
    <NavStack state={nav} class="flex-1">
      {(route) => {
        const Screen = screens[route.name];
        return <Screen />;
      }}
    </NavStack>
  );
}

mount(App);
