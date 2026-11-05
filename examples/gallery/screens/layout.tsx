import { signal } from "@nlayer/runtime";
import { NavPage, Scroll, Text, View } from "@nlayer/ui";
import { dismiss } from "../nav";
import { Section } from "../Section";

function Layout() {
  const tapped = signal(false);
  return (
    <Section title="View">
      <View class="flex w-40 flex-col gap-2">
        <View class="h-7 rounded-md bg-blue-100 p-1">
          <Text>One</Text>
        </View>
        <View class="h-7 rounded-md bg-blue-100 p-1">
          <Text>Two</Text>
        </View>
        <View class="h-7 rounded-md bg-blue-100 p-1">
          <Text>Three</Text>
        </View>
      </View>
      <View class="flex flex-row gap-2">
        <View class="h-7 w-16 rounded-md bg-slate-200 p-1">
          <Text>Left</Text>
        </View>
        <View class="h-7 w-16 rounded-md bg-slate-200 p-1">
          <Text>Right</Text>
        </View>
      </View>
      <View class="grid w-45 grid-cols-2 grid-rows-[32px_32px] gap-2">
        <View class="rounded-md bg-slate-100 p-1.5">
          <Text>A</Text>
        </View>
        <View class="rounded-md bg-slate-100 p-1.5">
          <Text>B</Text>
        </View>
        <View class="rounded-md bg-slate-100 p-1.5">
          <Text>C</Text>
        </View>
        <View class="rounded-md bg-slate-100 p-1.5">
          <Text>D</Text>
        </View>
      </View>
      <View onPress={() => tapped.set(true)} class="self-start rounded-lg bg-[#0f172a] p-2">
        <Text class="text-white">{tapped() ? "Pressed" : "Press"}</Text>
      </View>
      <View class="h-9 w-18 translate-x-3 rotate-15 scale-125 rounded-md bg-[#fde68a] p-1.5">
        <Text>Shifted</Text>
      </View>
      <View class="rounded-md bg-cyan-50 pt-[env(safe-area-inset-top)] pl-[env(safe-area-inset-left)]">
        <Text>Inset</Text>
      </View>
    </Section>
  );
}

export function ViewPage() {
  return (
    <NavPage title="View" onDismiss={dismiss}>
      <Scroll testID="view-page" class="flex-1 bg-slate-50">
        <View class="p-4">
          <Layout />
        </View>
      </Scroll>
    </NavPage>
  );
}
