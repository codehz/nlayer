import { signal } from "@nlayer/runtime";
import { NavPage, Scroll, Text, View, createAnimatedValue } from "@nlayer/ui";
import { dismiss } from "../nav";
import { Section } from "../Section";

function Animations() {
  const opacity = createAnimatedValue(1);
  const scale = createAnimatedValue(1);
  const longPressed = signal(false);

  const animate = () => {
    opacity.timing({ toValue: 0.3, duration: 300, easing: "ease-out" });
    scale.spring({ toValue: 1.2, stiffness: 180, damping: 12 });
    setTimeout(() => {
      opacity.timing({ toValue: 1, duration: 300 });
      scale.spring({ toValue: 1, stiffness: 180, damping: 12 });
    }, 300);
  };

  return (
    <Section title="Animations">
      <Text class="mb-2 text-sm text-[#64748b]">
        Tap to animate, long-press for different action.
      </Text>
      <View
        testID="animated-box"
        class="size-25 rounded-xl bg-[#3b82f6]"
        style={{
          opacity,
          transform: [{ scale }],
        }}
        onPress={animate}
        onLongPress={() => {
          longPressed.set(true);
          setTimeout(() => longPressed.set(false), 1000);
        }}
        delayLongPress={500}
      />
      {longPressed() && <Text class="mt-2 text-sm text-[#10b981]">Long pressed!</Text>}
    </Section>
  );
}

export function AnimationPage() {
  return (
    <NavPage title="Animations" onDismiss={dismiss}>
      <Scroll testID="animation-page" class="flex-1 bg-slate-50">
        <View class="pt-[calc(env(safe-area-inset-top)+16px)] pr-4 pb-4 pl-4">
          <Animations />
        </View>
      </Scroll>
    </NavPage>
  );
}
