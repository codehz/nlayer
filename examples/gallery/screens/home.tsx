import { NavPage, Scroll, Text, View } from "@nlayer/ui";
import { dismiss, dismissedCount, nav } from "../nav";
import { Section } from "../Section";

/** A row of the index: the screen it pushes, and the demo it holds. */
function Link(props: { testID: string; children: string; onPress: () => void }) {
  return (
    <Text testID={props.testID} onPress={props.onPress} class="p-2 font-semibold text-[#3b82f6]">
      {props.children}
    </Text>
  );
}

export function HomePage() {
  return (
    <NavPage title="Gallery" onDismiss={dismiss}>
      <Scroll testID="page" class="flex-1 bg-slate-50">
        <View
          testID="safe"
          class="pt-[calc(env(safe-area-inset-top)+16px)] pr-[calc(env(safe-area-inset-right)+16px)] pb-[calc(max(env(safe-area-inset-bottom),env(keyboard-inset-height))+32px)] pl-[calc(env(safe-area-inset-left)+16px)]"
        >
          <Text testID="home-title" class="text-[28px] font-bold text-[#0f172a]">
            Gallery
          </Text>
          <Text class="text-sm text-[#64748b]">v3 Navigation: Stack, Screens, Modals, Dialogs</Text>

          <Section title="Elements">
            <Link testID="open-view" onPress={() => nav.push({ name: "view" })}>
              → View (layout, press, transform)
            </Link>
            <Link testID="open-pointer" onPress={() => nav.push({ name: "pointer" })}>
              → Pointer (active, hover, focus)
            </Link>
            <Link testID="open-text" onPress={() => nav.push({ name: "text" })}>
              → Text (nested spans)
            </Link>
            <Link testID="open-image" onPress={() => nav.push({ name: "image" })}>
              → Image (bundled and remote)
            </Link>
            <Link testID="open-scroll" onPress={() => nav.push({ name: "scroll" })}>
              → Scroll (axes, scroll events)
            </Link>
            <Link testID="open-input" onPress={() => nav.push({ name: "input" })}>
              → Input (keyboards, password, disabled)
            </Link>
          </Section>

          <Section title="Platform APIs">
            <Link testID="open-fetch" onPress={() => nav.push({ name: "fetch" })}>
              → Fetch (quote, resource)
            </Link>
            <Link testID="open-platform" onPress={() => nav.push({ name: "platform" })}>
              → Platform (OS, color scheme, back handler)
            </Link>
            <Link testID="open-storage" onPress={() => nav.push({ name: "storage" })}>
              → Storage
            </Link>
            <Link testID="open-animation" onPress={() => nav.push({ name: "animation" })}>
              → Animations (opacity, spring, long press)
            </Link>
          </Section>

          <Section title="Navigation Demos">
            <Text testID="dismissed-count" class="mb-2 text-xs text-[#64748b]">
              Dismissed: {dismissedCount()}
            </Text>
            <Link testID="open-detail" onPress={() => nav.push({ name: "detail", id: 1 })}>
              → Detail (push/pop/onDismiss/onBackPress)
            </Link>
            <Link testID="open-dialog" onPress={() => nav.push({ name: "dialog" })}>
              → Dialog Demo
            </Link>
            <Link testID="open-gesture" onPress={() => nav.push({ name: "gesture" })}>
              → Gesture Card (pan → translate)
            </Link>
            <Link testID="open-list" onPress={() => nav.push({ name: "list" })}>
              → List Demo (5000 rows)
            </Link>
            <Link testID="open-virtual-list" onPress={() => nav.push({ name: "virtual-list" })}>
              → Virtual List (header, footer, separator)
            </Link>
            <Link testID="open-header" onPress={() => nav.push({ name: "header" })}>
              → Header Demo (title, headerStyle, headerTransparent)
            </Link>
            <Link
              testID="open-viewport-parent"
              onPress={() => nav.push({ name: "viewport-parent" })}
            >
              → List viewport="parent" (nested in Scroll)
            </Link>
            <Link testID="open-reading" onPress={() => nav.push({ name: "reading" })}>
              → Reading (selection, inline views, href)
            </Link>
            <Link testID="open-classes" onPress={() => nav.push({ name: "classes" })}>
              → Classes (utilities)
            </Link>
            <Link testID="open-effects" onPress={() => nav.push({ name: "effects" })}>
              → Effects (shadows, gradients)
            </Link>
          </Section>
        </View>
      </Scroll>
    </NavPage>
  );
}
