import { Image, Input, Scroll, Text, View } from "@nlayer/ui";
import mark from "../mark.png";
import { Section } from "../Section";

function ClassSample(props: { name: string; code: string; children?: unknown }) {
  return (
    <View class="flex flex-1 flex-col gap-1.5">
      <Text class="text-sm font-semibold text-[#0f172a]">{props.name}</Text>
      <Text class="text-xs text-[#64748b]">{props.code}</Text>
      {props.children}
    </View>
  );
}

export function ClassesPage() {
  return (
    <Scroll testID="classes-page" class="flex-1 bg-slate-50">
      <View class="pt-[calc(env(safe-area-inset-top)+16px)] pr-[calc(env(safe-area-inset-right)+16px)] pb-[calc(max(env(safe-area-inset-bottom),env(keyboard-inset-height))+32px)] pl-[calc(env(safe-area-inset-left)+16px)]">
        <Text class="text-[28px] font-bold text-[#0f172a]">Classes</Text>
        <Text class="text-sm text-[#64748b]">
          v5: layout, color, state, and names that only warn
        </Text>

        <Section title="State">
          <View class="flex flex-row gap-3">
            <ClassSample name="Opacity" code="opacity-50">
              <View class="flex flex-row items-center gap-2 self-start rounded-lg bg-[#0f172a] p-3">
                <View class="size-8 rounded-md bg-white" />
                <View testID="class-faded" class="size-8 rounded-md bg-white opacity-50" />
              </View>
            </ClassSample>
            <ClassSample name="Translate" code="translate-x-1">
              <View class="self-start rounded-lg bg-slate-100 p-3">
                <View class="flex flex-row items-start">
                  <View class="size-5 rounded bg-slate-300" />
                  <View
                    testID="class-shift"
                    class="-ml-5 size-5 translate-x-1 rounded bg-[#0f172a]"
                  />
                </View>
              </View>
            </ClassSample>
          </View>
          <View class="flex flex-row gap-3">
            <ClassSample name="Hover" code="hover:p-4">
              <View testID="class-hover" class="self-start rounded-lg bg-[#dbeafe] hover:p-4">
                <View testID="hover-child" class="rounded-lg bg-[#0f172a] px-3 py-2">
                  <Text class="text-xs font-semibold text-white">Hover</Text>
                </View>
              </View>
            </ClassSample>
            <ClassSample name="Press" code="active:scale-95">
              <View
                testID="class-press"
                onPress={() => {}}
                class="flex size-16 items-center justify-center self-start rounded-2xl bg-[#0f172a] active:scale-95"
              >
                <Text class="text-xs font-semibold text-white">Press</Text>
              </View>
            </ClassSample>
          </View>
          <ClassSample name="Focus" code="focus-within:p-4 focus-visible:opacity-50">
            <View testID="class-within" class="rounded-lg bg-slate-100 focus-within:p-4">
              <Input
                testID="class-field"
                value=""
                placeholder="Focus"
                onChange={() => {}}
                class="rounded-lg border border-[#cbd5e1] bg-white p-2 text-base text-[#0f172a] focus:p-2 focus-visible:opacity-50"
              />
            </View>
          </ClassSample>
          <ClassSample name="Disabled" code="disabled:opacity-25 enabled:opacity-50">
            <View class="flex flex-row gap-2">
              <View class="flex flex-1 flex-col items-center gap-2 rounded-lg bg-slate-100 py-4">
                <View
                  testID="class-disabled"
                  disabled
                  class="size-12 rounded-xl bg-[#0f172a] disabled:opacity-25 enabled:opacity-50"
                />
                <Text class="text-xs text-[#64748b]">disabled</Text>
              </View>
              <View class="flex flex-1 flex-col items-center gap-2 rounded-lg bg-slate-100 py-4">
                <View
                  testID="class-enabled"
                  class="size-12 rounded-xl bg-[#0f172a] disabled:opacity-25 enabled:opacity-50"
                />
                <Text class="text-xs text-[#64748b]">enabled</Text>
              </View>
            </View>
          </ClassSample>
        </Section>

        <Section title="Layout">
          <ClassSample name="Flex" code="flex-row items-start gap-3 p-4">
            <View
              testID="class-row"
              class="flex flex-row items-start gap-3 self-start rounded-lg bg-zinc-800 p-4"
            >
              <View testID="class-a" class="size-2.5 rounded-full bg-white" />
              <View testID="class-b" class="size-2.5 rounded-full bg-white" />
            </View>
          </ClassSample>
          <View class="flex flex-row gap-3">
            <ClassSample name="Space" code="space-x-2">
              <View class="self-start rounded-lg bg-slate-100 p-3">
                <View testID="class-space" class="flex flex-row items-start space-x-2">
                  <View testID="space-a" class="size-2 rounded-full bg-[#0f172a]" />
                  <View testID="space-b" class="size-2 rounded-full bg-[#0f172a]" />
                </View>
              </View>
            </ClassSample>
            <ClassSample name="Arbitrary" code="h-[11px] w-[37px]">
              <View class="self-start rounded-lg bg-slate-100 p-3">
                <View
                  testID="class-arbitrary"
                  class="h-[11px] w-[37px] rounded-full bg-[#7c3aed]"
                />
              </View>
            </ClassSample>
          </View>
        </Section>

        <Section title="Type">
          <View class="flex flex-row items-center gap-3">
            <Image testID="class-image" src={mark} class="size-16 rounded-lg object-cover" />
            <ClassSample name="Label" code="text-sm font-semibold text-zinc-800">
              <Text testID="class-label" class="text-sm font-semibold text-zinc-800">
                Cover mark
              </Text>
            </ClassSample>
          </View>
          <ClassSample name="Clamp" code="line-clamp-2">
            <View class="self-start rounded-lg bg-slate-50 p-3">
              <Text testID="class-clamp" class="w-40 text-sm leading-5 text-[#334155] line-clamp-2">
                one two three four five six seven eight nine ten
              </Text>
            </View>
          </ClassSample>
          <ClassSample name="Selection" code="selection:bg-zinc-200">
            <Text
              testID="class-select"
              class="text-base leading-6 text-[#0f172a] select-text selection:bg-zinc-200"
            >
              Select this sentence. The highlight is zinc-200.
            </Text>
          </ClassSample>
        </Section>

        <Section title="Precedence">
          <View class="flex flex-row gap-2">
            <ClassSample name="Important" code="!p-4 beats paddingTop">
              <View
                testID="class-important"
                class="flex flex-col rounded-lg bg-slate-100 !p-4"
                style={{ paddingTop: 1 }}
              >
                <View testID="important-child" class="h-8 rounded-md bg-[#0f172a]" />
              </View>
            </ClassSample>
            <ClassSample name="Style" code="paddingLeft replaces p-4">
              <View
                testID="class-override"
                class="flex flex-col rounded-lg bg-slate-100 p-4"
                style={{ paddingLeft: 8 }}
              >
                <View testID="override-child" class="h-8 rounded-md bg-[#7c3aed]" />
              </View>
            </ClassSample>
          </View>
          <ClassSample name="Warned" code="p-4 applies, blur-sm and wat do not">
            <View
              testID="class-mixed"
              class="flex flex-col gap-2 rounded-lg bg-amber-50 p-4 blur-sm wat"
            >
              <View testID="mixed-child" class="size-2 rounded-full bg-amber-800" />
              <Text class="text-xs leading-5 text-amber-900">
                Padding stays. blur-sm is unsupported and wat is unknown.
              </Text>
            </View>
          </ClassSample>
        </Section>
      </View>
    </Scroll>
  );
}
