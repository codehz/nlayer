import { mount } from "@nlayer/runtime";
import { NavPage, NavStack, Text, View } from "@nlayer/ui";
import { detailBackIntercepted, detailInterceptBack, dismissedCount, nav, type Route } from "./nav";
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

/** Web draws this (`header`). iOS and Android keep the system bar. */
function PageHeader(props: { title: string }) {
  return (
    <View class="h-11 flex-row items-center gap-3 border-b border-b-slate-200 bg-white px-3">
      <Text onPress={() => nav.pop()} class="font-semibold text-[#3b82f6]">
        ← Back
      </Text>
      <Text class="text-[15px] font-semibold text-[#0f172a]">{props.title}</Text>
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
      // iOS and Android put these in the system header; web draws
      // `header` at the top of the page instead.
      headerLeft: (
        <Text onPress={() => nav.pop()} class="font-semibold text-white">
          Back
        </Text>
      ),
      headerRight: <Text class="text-white">Right</Text>,
      header: (
        <View
          testID="drawn-header"
          class="h-11 flex-row items-center justify-between bg-[#10b981] px-3"
        >
          <Text
            testID="drawn-header-back"
            onPress={() => nav.pop()}
            class="font-semibold text-white"
          >
            ← Back
          </Text>
          <Text class="font-bold text-white">Header</Text>
          <Text class="text-white">Right</Text>
        </View>
      ),
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
      {(route) => {
        const screen = page(route);
        const header =
          screen.header ??
          (route.name !== "home" && screen.title ? <PageHeader title={screen.title} /> : undefined);
        return (
          <NavPage
            title={screen.title}
            headerStyle={screen.headerStyle}
            headerTransparent={false}
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
