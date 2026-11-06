import { signal } from "@nlayer/runtime";
import { List, NavPage, Text, View } from "@nlayer/ui";
import { dismiss } from "../nav";

export function ListDemoPage() {
  const loaded = signal(1000);
  const refreshing = signal(false);
  const snapEnabled = signal(false);
  const stickyEnabled = signal(false);
  const invertedEnabled = signal(false);
  const viewableFirst = signal(0);
  const viewableLast = signal(0);

  const handleRefresh = () => {
    refreshing.set(true);
    setTimeout(() => {
      refreshing.set(false);
    }, 1000);
  };

  return (
    <NavPage title="Virtual List" onDismiss={dismiss}>
      <View testID="list-page" class="flex-1 bg-white">
        <View class="border-b border-b-slate-200 pt-[calc(env(safe-area-inset-top)+16px)] pr-4 pb-4 pl-4">
          <Text class="text-xl font-bold">Virtual List</Text>

          <View class="mt-2 flex flex-col gap-2">
            <Text
              testID="toggle-snap"
              onPress={() => snapEnabled.set(!snapEnabled())}
              class="rounded p-1.5 text-xs bg-[#cbd5e1]"
              classList={{ "bg-[#10b981]": snapEnabled() }}
            >
              snapToInterval: {snapEnabled() ? "64px" : "off"}
            </Text>
            <Text
              testID="toggle-sticky"
              onPress={() => stickyEnabled.set(!stickyEnabled())}
              class="rounded p-1.5 text-xs bg-[#cbd5e1]"
              classList={{ "bg-[#10b981]": stickyEnabled() }}
            >
              stickyHeaderIndices: {stickyEnabled() ? "[0, 10, 20]" : "off"}
            </Text>
            <Text
              testID="toggle-inverted"
              onPress={() => invertedEnabled.set(!invertedEnabled())}
              class="rounded p-1.5 text-xs bg-[#cbd5e1]"
              classList={{ "bg-[#10b981]": invertedEnabled() }}
            >
              inverted: {invertedEnabled() ? "true" : "false"}
            </Text>
            <Text testID="viewable-range" class="text-[11px] text-[#64748b]">
              Viewable: {viewableFirst()} - {viewableLast()}
            </Text>
          </View>
        </View>
        <List
          testID="list"
          class="flex-1"
          each={Array.from({ length: loaded() }, (_, i) => i)}
          estimatedItemSize={32}
          refreshing={refreshing()}
          onRefresh={handleRefresh}
          snapToInterval={snapEnabled() ? 64 : undefined}
          stickyHeaderIndices={stickyEnabled() ? [0, 10, 20] : undefined}
          inverted={invertedEnabled()}
          onViewableItemsChange={(e) => {
            viewableFirst.set(e.first);
            viewableLast.set(e.last);
          }}
          onLoadMore={() => {
            if (loaded() < 5000) {
              loaded.set(Math.min(5000, loaded() + 1000));
            }
          }}
        >
          {(index) => (
            <View
              testID={index() === 0 ? "first-item" : undefined}
              class="h-8 justify-center border-b border-b-slate-100 bg-white px-4"
              classList={{
                "bg-[#e0f2fe]": [0, 10, 20].includes(index()) && stickyEnabled(),
              }}
            >
              <Text testID={index() === 0 ? "first" : undefined}>
                Row {index()} {[0, 10, 20].includes(index()) && stickyEnabled() ? "(sticky)" : ""}
              </Text>
            </View>
          )}
        </List>
      </View>
    </NavPage>
  );
}
