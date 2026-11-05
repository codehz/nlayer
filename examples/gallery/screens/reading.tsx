import { signal } from "@nlayer/runtime";
import { Image, Scroll, Text, View } from "@nlayer/ui";
import mark from "../mark.png";
import { Section } from "../Section";

export function ReadingPage() {
  const pressed = signal(0);
  return (
    <Scroll testID="reading-page" class="flex-1 bg-slate-50">
      <View class="pt-[calc(env(safe-area-inset-top)+16px)] pr-[calc(env(safe-area-inset-right)+16px)] pb-8 pl-[calc(env(safe-area-inset-left)+16px)]">
        <Text class="text-[28px] font-bold text-[#0f172a]">Reading</Text>
        <Text class="text-sm text-[#64748b]">v4: selectable text, inline views, href links</Text>

        <Section title="Selectable paragraph">
          <Text
            testID="reading-selectable-paragraph"
            class="text-base leading-[26px] text-[#0f172a] select-text selection:bg-[#93c5fd]"
          >
            {"Long-press a word here to select it"}
            <Text class="font-bold">{" — and this nested span comes too"}</Text>
            {". The handles extend and the system menu copies it."}
          </Text>
        </Section>

        <Section title="Selectable view">
          <View testID="reading-selectable" selectable class="flex flex-col gap-3">
            <Text
              testID="reading-para-one"
              class="text-base leading-[26px] text-[#0f172a] selection:bg-[#fde68a]"
            >
              A selection started here continues through the paragraphs below, in reading order.
            </Text>
            <Text
              testID="reading-none-label"
              class="select-none text-xs font-semibold text-[#64748b]"
            >
              Caption — not selectable
            </Text>
            <Text
              testID="reading-para-two"
              class="text-base leading-[26px] text-[#0f172a] selection:bg-[#fde68a]"
            >
              So this paragraph joins the same selection, while the caption between them is skipped.
            </Text>
          </View>
        </Section>

        <Section title="Inline view and image">
          <Text testID="reading-inline" class="text-base leading-[26px] text-[#0f172a]">
            {"Made with "}
            <Image src={mark} class="size-4.5 rounded align-text-top" />
            {" and shipped "}
            <View
              testID="reading-inline-badge"
              class="h-4.5 justify-center rounded-[9px] bg-green-100 px-1.75 align-middle"
            >
              <Text class="text-[10px] font-bold text-[#166534]">LIVE</Text>
            </View>
            {" today."}
          </Text>
        </Section>

        <Section title="Link">
          <Text
            testID="reading-terms-parent"
            onPress={() => pressed.set(pressed() + 1)}
            class="text-[15px] leading-6 text-[#0f172a]"
          >
            {"By continuing you agree to the "}
            <Text
              testID="reading-terms"
              href="https://example.com/terms"
              onPress={() => pressed.set(pressed() + 1)}
              class="text-[#2563eb]"
              style={{ textDecorationLine: "underline" }}
            >
              Terms of service
            </Text>
            {"."}
          </Text>
          <Text testID="reading-press-count" class="text-xs text-[#64748b]">
            Link pressed: {pressed()}
          </Text>
        </Section>
      </View>
    </Scroll>
  );
}
