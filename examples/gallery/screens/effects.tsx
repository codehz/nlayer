import { NavPage, Scroll, Text, View } from "@nlayer/ui";
import { dismiss } from "../nav";
import { Section } from "../Section";

export function EffectsPage() {
  return (
    <NavPage title="Effects" onDismiss={dismiss}>
      <Scroll testID="effects-page" class="flex-1 bg-slate-50">
        <View class="pt-[calc(env(safe-area-inset-top)+16px)] pr-[calc(env(safe-area-inset-right)+16px)] pb-[calc(max(env(safe-area-inset-bottom),env(keyboard-inset-height))+32px)] pl-[calc(env(safe-area-inset-left)+16px)]">
          <Text class="text-[28px] font-bold text-[#0f172a]">Effects</Text>
          <Text class="text-sm text-[#64748b]">v7: shadows and gradients</Text>

          <Section title="Shadows">
            <View class="flex flex-row flex-wrap gap-3">
              <View class="flex flex-col gap-1.5">
                <Text class="text-xs text-[#64748b]">outer, clipped</Text>
                <View
                  testID="fx-shadow"
                  class="h-16 w-24 flex-none rounded-2xl bg-white"
                  style={{
                    boxShadow:
                      "0 8px 24px 4px rgba(15,23,42,0.28), 0 2px 4px -1px rgba(15,23,42,0.2)",
                    overflow: "hidden",
                  }}
                >
                  <View class="h-24 w-24 flex-none bg-[#bfdbfe]" />
                </View>
              </View>
              <View class="flex flex-col gap-1.5">
                <Text class="text-xs text-[#64748b]">inset</Text>
                <View
                  testID="fx-inset"
                  class="h-16 w-24 flex-none rounded-2xl bg-[#f8fafc]"
                  style={{ boxShadow: "inset 0 2px 8px rgba(15,23,42,0.35)" }}
                />
              </View>
            </View>
            <View class="flex flex-row flex-wrap gap-3">
              <View
                testID="fx-state"
                class="h-16 w-24 flex-none rounded-2xl bg-white"
                style={{ boxShadow: "0 1px 2px rgba(15,23,42,0.2)" }}
                hoverStyle={{ boxShadow: "0 12px 32px rgba(15,23,42,0.3)" }}
              />
              <View
                testID="fx-class-hover"
                class="h-16 w-24 flex-none rounded-2xl bg-white shadow-sm hover:shadow-lg"
              />
              <View
                testID="fx-class-hover-none"
                class="h-16 w-24 flex-none rounded-2xl bg-white shadow-md hover:shadow-none"
              />
            </View>
          </Section>

          <Section title="Gradients">
            <View class="flex flex-row flex-wrap gap-3">
              <View
                testID="fx-gradient"
                class="h-16 w-24 flex-none rounded-xl"
                style={{
                  backgroundColor: "#fef3c7",
                  backgroundImage: "linear-gradient(90deg, rgba(59,130,246,0) 0%, #3b82f6 100%)",
                }}
              />
              <View
                testID="fx-radial"
                class="h-16 w-24 flex-none rounded-xl"
                style={{
                  backgroundImage: "radial-gradient(circle at 30% 30%, #f59e0b, #ef4444 70%)",
                }}
              />
              <View
                testID="fx-conic"
                class="h-16 w-16 flex-none rounded-xl"
                style={{
                  backgroundImage:
                    "conic-gradient(from 0deg, #ef4444, #eab308, #22c55e, #3b82f6, #ef4444)",
                }}
              />
            </View>
          </Section>

          <Section title="Classes">
            <View class="flex flex-row flex-wrap gap-3 rounded-lg bg-slate-100 p-3">
              <View
                testID="fx-class-shadow"
                class="h-16 w-16 flex-none rounded-xl bg-white shadow-md blur-sm"
              />
              <View
                testID="fx-class-gradient"
                class="h-16 w-16 flex-none rounded-xl bg-linear-to-r from-red-500 via-green-500 to-blue-500"
              />
              <View
                testID="fx-class-ring"
                class="h-16 w-16 flex-none rounded-xl bg-white ring-2 ring-blue-500 ring-offset-2"
              />
              <View
                testID="fx-class-inset"
                class="h-16 w-16 flex-none rounded-xl bg-white inset-shadow-sm"
              />
            </View>
            <Text class="text-xs leading-5 text-[#64748b]">shadow-md draws. blur-sm does not.</Text>
          </Section>
        </View>
      </Scroll>
    </NavPage>
  );
}
