import { NavPage, Scroll, Text, View } from "@nlayer/ui";
import { dismiss } from "../nav";
import { Section } from "../Section";

function Type() {
  return (
    <Section title="Text">
      <Text class="text-base text-[#0f172a]">
        Plain
        <Text class="font-bold"> bold</Text>
        <Text class="italic"> italic</Text>
        <Text class="text-[22px] text-[#c2410c]"> color</Text>
      </Text>
    </Section>
  );
}

export function TextPage() {
  return (
    <NavPage title="Text" onDismiss={dismiss}>
      <Scroll testID="text-page" class="flex-1 bg-slate-50">
        <View class="pt-[calc(env(safe-area-inset-top)+16px)] pr-4 pb-4 pl-4">
          <Type />
        </View>
      </Scroll>
    </NavPage>
  );
}
