import { effect, signal, storage } from "@nlayer/runtime";
import { Input, NavPage, Scroll, Text, View } from "@nlayer/ui";
import { dismiss } from "../nav";
import { Section } from "../Section";

function StorageDemo() {
  const storedValue = signal("");
  const inputValue = signal("");

  effect(() => {
    storage.getItem("demo-key").then((value) => {
      storedValue.set(value || "(empty)");
    });
  });

  const save = () => {
    storage.setItem("demo-key", inputValue()).then(() => {
      storedValue.set(inputValue());
    });
  };

  const clear = () => {
    storage.removeItem("demo-key").then(() => {
      storedValue.set("(empty)");
      inputValue.set("");
    });
  };

  return (
    <Section title="Storage">
      <Text class="mb-2 text-sm text-[#64748b]">Stored: {storedValue()}</Text>
      <Input
        testID="storage-input"
        value={inputValue()}
        onChange={({ value }) => inputValue.set(value)}
        placeholder="Enter value to store"
        class="rounded-md border border-[#cbd5e1] p-2"
      />
      <View class="mt-2 flex flex-row gap-2">
        <View testID="save-storage" class="rounded-md bg-[#3b82f6] p-2" onPress={save}>
          <Text class="text-xs text-white">Save</Text>
        </View>
        <View testID="clear-storage" class="rounded-md bg-[#ef4444] p-2" onPress={clear}>
          <Text class="text-xs text-white">Clear</Text>
        </View>
      </View>
    </Section>
  );
}

export function StoragePage() {
  return (
    <NavPage title="Storage" onDismiss={dismiss}>
      <Scroll testID="storage-page" class="flex-1 bg-slate-50">
        <View class="pt-[calc(env(safe-area-inset-top)+16px)] pr-4 pb-4 pl-4">
          <StorageDemo />
        </View>
      </Scroll>
    </NavPage>
  );
}
