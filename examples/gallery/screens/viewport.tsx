import { List, NavPage, Scroll, Text, View } from "@nlayer/ui";
import { dismiss } from "../nav";

export function ViewportParentPage() {
  return (
    <NavPage title="Viewport Parent" onDismiss={dismiss}>
      <View testID="viewport-parent-page" class="flex-1 min-h-0 bg-white">
        <View class="border-b border-b-slate-200 p-4">
          <Text class="text-xl font-bold">List viewport="parent"</Text>
          <Text class="mt-2 text-xs text-[#64748b]">
            List uses outer Scroll's viewport (no nested scroll)
          </Text>
        </View>
        <Scroll testID="outer-scroll" class="flex-1">
          <View class="p-4">
            <Text class="mb-2 text-sm font-semibold">Content before list</Text>
          </View>
          <List
            testID="vp-list"
            viewport="parent"
            each={Array.from({ length: 100 }, (_, i) => i)}
            estimatedItemSize={40}
          >
            {(index) => (
              <View
                testID={`vp-item-${index()}`}
                class="h-10 justify-center border-b border-b-slate-100 px-4"
              >
                <Text>Item {index()}</Text>
              </View>
            )}
          </List>
          <View class="p-4">
            <Text class="mt-2 text-sm font-semibold">Content after list</Text>
          </View>
        </Scroll>
      </View>
    </NavPage>
  );
}
