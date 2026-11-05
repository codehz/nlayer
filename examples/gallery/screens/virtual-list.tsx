import { signal } from "@nlayer/runtime";
import { List, NavPage, Scroll, Text, View } from "@nlayer/ui";
import { dismiss } from "../nav";
import { Section } from "../Section";

function VirtualList() {
  const items = Array.from({ length: 5000 }, (_, i) => ({ id: i, title: `Item ${i}` }));
  const count = signal(items.length);
  let listRef:
    | {
        scrollToIndex: (index: number, options?: { animated?: boolean }) => void;
        scrollToEnd: (options?: { animated?: boolean }) => void;
      }
    | undefined;

  return (
    <Section title="List">
      <Text class="mb-2 text-sm text-[#64748b]">
        {count()} items, recycled. Scroll to see more.
      </Text>
      <View class="h-75 rounded-lg border border-slate-200">
        <List
          each={items.slice(0, count())}
          estimatedItemSize={44}
          header={
            <View class="bg-slate-100 p-3">
              <Text class="font-semibold">Header</Text>
            </View>
          }
          footer={
            <View class="bg-slate-100 p-3">
              <Text class="font-semibold">Footer</Text>
            </View>
          }
          separator={<View class="h-px bg-slate-200" />}
          onLoadMore={() => {
            if (count() < items.length) {
              count.set(Math.min(count() + 50, items.length));
            }
          }}
          ref={(handle) => (listRef = handle)}
          class="flex-1"
        >
          {(item, index) => (
            <View class="p-3">
              <Text class="text-sm text-[#0f172a]">
                {item().title} (index: {index()})
              </Text>
            </View>
          )}
        </List>
      </View>
      <View class="mt-2 flex flex-row gap-2">
        <View
          testID="scroll-to-end"
          class="rounded-md bg-[#3b82f6] p-2"
          onPress={() => listRef?.scrollToEnd({ animated: true })}
        >
          <Text class="text-xs text-white">Scroll to End</Text>
        </View>
        <View
          testID="scroll-to-top"
          class="rounded-md bg-[#3b82f6] p-2"
          onPress={() => listRef?.scrollToIndex(0, { animated: true })}
        >
          <Text class="text-xs text-white">Scroll to Top</Text>
        </View>
      </View>
    </Section>
  );
}

export function VirtualListPage() {
  return (
    <NavPage title="Virtual List" onDismiss={dismiss}>
      <Scroll testID="virtual-list-page" class="flex-1 bg-slate-50">
        <View class="p-4">
          <VirtualList />
        </View>
      </Scroll>
    </NavPage>
  );
}
