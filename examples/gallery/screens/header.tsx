import { NavPage, Scroll, Text, View } from "@nlayer/ui";
import { dismiss, headerLargeTitle, headerSupplement, headerTransparent, nav } from "../nav";

/** The v11 bar: item groups (a custom view and buttons), the back button's
 * ownership, a subtitle, the two appearances and the static large title. The
 * hosts draw it: the system bar on iOS, a drawn bar on Android and the web. */
export function HeaderDemoPage() {
  return (
    <NavPage
      title="Header"
      subtitle="v11 bar"
      largeTitleDisplayMode={headerLargeTitle() ? "always" : "never"}
      standardAppearance={{
        backgroundColor: headerTransparent() ? "transparent" : "#10b981",
        titleColor: "#fff",
        tintColor: "#fff",
      }}
      scrollEdgeAppearance={{ backgroundColor: "transparent", shadowColor: "none" }}
      leftItemsSupplementBackButton={headerSupplement()}
      leadingItemGroups={[
        [
          <Text onPress={() => nav.pop()} class="font-semibold text-white">
            Back
          </Text>,
        ],
        [{ title: "Edit", onPress: () => nav.pop() }],
      ]}
      trailingItemGroups={[
        [
          { title: "Share", onPress: () => {} },
          { title: "More", onPress: () => {} },
        ],
        [<Text class="text-white">Right</Text>],
      ]}
      onDismiss={dismiss}
    >
      <Scroll
        testID="header-demo"
        class="flex-1 bg-white"
        classList={{ "bg-[#3b82f6]": headerTransparent() }}
      >
        <View class="p-4 pt-[calc(env(safe-area-inset-top)+16px)]">
          <Text
            class="mb-3 text-2xl font-bold text-black"
            classList={{ "text-white": headerTransparent() }}
          >
            Header Demo
          </Text>
          <Text class="mb-4 text-[#64748b]" classList={{ "text-sky-50": headerTransparent() }}>
            Two leading groups and two trailing groups; the leading ones replace the back button
            unless supplement is on. A subtitle, a transparent bar, and a static large title.
          </Text>
          <Text
            testID="toggle-transparent"
            onPress={() => headerTransparent.set(!headerTransparent())}
            class="mb-4 rounded-md bg-[#3b82f6] p-3 text-center text-white"
            classList={{ "bg-[#1e40af]": headerTransparent() }}
          >
            {headerTransparent() ? "Transparent ON" : "Toggle Transparent"}
          </Text>
          <Text
            testID="toggle-large-title"
            onPress={() => headerLargeTitle.set(!headerLargeTitle())}
            class="mb-4 rounded-md bg-[#3b82f6] p-3 text-center text-white"
            classList={{ "bg-[#1e40af]": headerLargeTitle() }}
          >
            {headerLargeTitle() ? "Large Title ON" : "Toggle Large Title"}
          </Text>
          <Text
            testID="toggle-supplement"
            onPress={() => headerSupplement.set(!headerSupplement())}
            class="mb-4 rounded-md bg-[#3b82f6] p-3 text-center text-white"
            classList={{ "bg-[#1e40af]": headerSupplement() }}
          >
            {headerSupplement() ? "Supplement Back ON" : "Toggle Supplement Back"}
          </Text>
        </View>
      </Scroll>
    </NavPage>
  );
}
