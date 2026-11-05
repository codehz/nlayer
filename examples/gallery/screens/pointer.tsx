import { signal } from "@nlayer/runtime";
import { Input, NavPage, Scroll, Text, View } from "@nlayer/ui";
import { dismiss } from "../nav";
import { Section } from "../Section";

function Pointer() {
  const faded = signal(false);
  const lit = signal(false);
  const focused = signal("");
  return (
    <Section title="Pointer">
      <View
        onPress={() => faded.set(true)}
        activeStyle={{ opacity: 0.4, width: 10 }}
        class="h-9 w-40 justify-center rounded-lg bg-[#0f172a] p-2"
      >
        <Text class="text-white">{faded() ? "Faded" : "Fade"}</Text>
      </View>
      <Text class="text-xs text-[#64748b]">{"activeStyle={{ opacity: 0.4 }}"}</Text>
      <View
        onPress={() => lit.set(true)}
        activeStyle={{ backgroundColor: "#bae6fd" }}
        class="self-start rounded-lg bg-slate-200 p-2"
      >
        <Text>{lit() ? "Lit" : "Light"}</Text>
      </View>
      <Text class="text-xs text-[#64748b]">{"activeStyle={{ backgroundColor }}"}</Text>
      <View
        hoverStyle={{ backgroundColor: "#dbeafe", width: 10 }}
        class="h-9 w-30 justify-center rounded-lg bg-slate-100 p-2"
      >
        <Text>Hover</Text>
      </View>
      <Text class="text-xs text-[#64748b]">{"hoverStyle={{ backgroundColor }}"}</Text>
      <Input
        value={focused()}
        placeholder="Focus"
        onChange={({ value }) => focused.set(value)}
        focusStyle={{ backgroundColor: "#ecfeff", color: "#0f172a", width: 10 }}
        class="w-40 rounded-lg border border-[#cbd5e1] bg-white p-2 text-base text-[#0f172a]"
      />
      <Text class="text-xs text-[#64748b]">{"focusStyle={{ backgroundColor }}"}</Text>
    </Section>
  );
}

export function PointerPage() {
  return (
    <NavPage title="Pointer" onDismiss={dismiss}>
      <Scroll testID="pointer-page" class="flex-1 bg-slate-50">
        <View class="p-4">
          <Pointer />
        </View>
      </Scroll>
    </NavPage>
  );
}
