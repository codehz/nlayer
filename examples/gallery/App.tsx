import { mount } from "@nlayer/runtime";
import { NavPage, NavStack, Text, View } from "@nlayer/ui";
import {
  detailBackIntercepted,
  detailInterceptBack,
  dismissedCount,
  headerTransparent,
  nav,
  type Route,
} from "./nav";
import { ClassesPage } from "./screens/classes";
import { DetailPage } from "./screens/detail";
import { DialogDemoPage } from "./screens/dialog";
import { EffectsPage } from "./screens/effects";
import { GestureCardPage } from "./screens/gesture";
import { HeaderDemoPage } from "./screens/header";
import { HomePage } from "./screens/home";
import { ListDemoPage } from "./screens/list";
import { ReadingPage } from "./screens/reading";
import { ViewportParentPage } from "./screens/viewport";

/** Web draws this (`header`). iOS and Android keep the system bar.
 * Its nodes are not the `headerLeft` / `headerRight` slots: the web does
 * not place those, and one node cannot sit in both. */
function PageHeader(props: {
  title: string;
  /** Bottom of the stack: no back control. */
  root?: boolean;
  /** Header demo: leading and trailing controls, like the native slots. */
  slots?: boolean;
  background?: string;
  color?: string;
  tint?: string;
  transparent?: boolean;
}) {
  const tint = props.transparent ? "#fff" : (props.tint ?? "#3b82f6");
  const color = props.transparent ? "#fff" : (props.color ?? "#0f172a");
  const hairline = !props.transparent && props.background == null;
  const leading = props.slots ? (
    <Text onPress={() => nav.pop()} class="font-semibold" style={{ color: tint }}>
      Back
    </Text>
  ) : props.root ? null : (
    <Text
      testID="drawn-header-back"
      onPress={() => nav.pop()}
      class="font-semibold"
      style={{ color: tint }}
    >
      ← Back
    </Text>
  );
  const trailing = props.slots ? (
    <Text class="font-semibold" style={{ color: tint }}>
      Right
    </Text>
  ) : null;
  return (
    <View
      testID="drawn-header"
      style={{
        paddingTop: "env(safe-area-inset-top)",
        backgroundColor: props.transparent ? "transparent" : (props.background ?? "#fff"),
      }}
    >
      <View
        class="h-11 flex-row items-center gap-2 px-3"
        style={{
          borderBottomWidth: hairline ? 1 : 0,
          borderBottomColor: "#e2e8f0",
        }}
      >
        {props.slots ? (
          <View class="w-20 flex-row items-center px-3">{leading}</View>
        ) : (
          leading
        )}
        <Text
          class="min-w-0 flex-1 text-[15px] font-semibold"
          classList={{ "text-center": props.slots }}
          style={{ numberOfLines: 1, color }}
        >
          {props.title}
        </Text>
        {props.slots ? (
          <View class="w-20 flex-row items-center justify-end px-3">{trailing}</View>
        ) : null}
      </View>
    </View>
  );
}

function page(route: Route) {
  if (route.name === "home") return { content: <HomePage />, title: "Gallery" };
  if (route.name === "detail") {
    return {
      content: <DetailPage />,
      title: "Detail",
      headerStyle: { backgroundColor: "#3b82f6", color: "#fff", tintColor: "#fff" },
    };
  }
  if (route.name === "dialog") return { content: <DialogDemoPage />, title: "Dialog" };
  if (route.name === "gesture") return { content: <GestureCardPage />, title: "Gestures" };
  if (route.name === "list") return { content: <ListDemoPage />, title: "Virtual List" };
  if (route.name === "header") {
    return {
      content: <HeaderDemoPage />,
      title: "Header",
      headerStyle: { backgroundColor: "#10b981", color: "#fff", tintColor: "#fff" },
      headerLeft: (
        <Text onPress={() => nav.pop()} class="font-semibold text-white">
          Back
        </Text>
      ),
      headerRight: <Text class="text-white">Right</Text>,
    };
  }
  if (route.name === "viewport-parent") {
    return { content: <ViewportParentPage />, title: "Viewport Parent" };
  }
  if (route.name === "reading") return { content: <ReadingPage />, title: "Reading" };
  if (route.name === "classes") return { content: <ClassesPage />, title: "Classes" };
  if (route.name === "effects") return { content: <EffectsPage />, title: "Effects" };
  return { content: <Text>Unknown route</Text> };
}

function App() {
  return (
    <NavStack state={nav} class="flex-1">
      {(route, index) => {
        const screen = page(route);
        const transparent = route.name === "header" && headerTransparent();
        const header = (
          <PageHeader
            title={screen.title ?? ""}
            root={index() === 0}
            slots={route.name === "header"}
            background={transparent ? undefined : screen.headerStyle?.backgroundColor}
            color={screen.headerStyle?.color}
            tint={screen.headerStyle?.tintColor}
            transparent={transparent}
          />
        );
        return (
          <NavPage
            title={screen.title}
            headerStyle={screen.headerStyle}
            headerTransparent={transparent}
            header={header}
            headerLeft={screen.headerLeft}
            headerRight={screen.headerRight}
            onDismiss={() => dismissedCount.set(dismissedCount() + 1)}
            popOnBack={route.name !== "detail" || !detailInterceptBack()}
            onBackPress={
              route.name === "detail"
                ? () => {
                    if (detailInterceptBack()) {
                      detailBackIntercepted.set(true);
                      setTimeout(() => detailBackIntercepted.set(false), 1000);
                    }
                  }
                : undefined
            }
          >
            {screen.content}
          </NavPage>
        );
      }}
    </NavStack>
  );
}

mount(App);
