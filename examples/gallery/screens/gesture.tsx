import { signal } from "@nlayer/runtime";
import { NavPage, Scroll, Text, View, createAnimatedValue } from "@nlayer/ui";
import { dismiss } from "../nav";

export function GestureCardPage() {
  const translateX = createAnimatedValue(0);
  const translateY = createAnimatedValue(0);
  const panEndX = signal(0);
  const panEndY = signal(0);

  const scale = createAnimatedValue(1);
  const pinchEndScale = signal(1);

  const angle = createAnimatedValue(0);
  const rotationEndAngle = signal(0);

  return (
    <NavPage title="Gestures" onDismiss={dismiss}>
      <Scroll testID="gesture-page" class="flex-1 bg-white">
        <View class="pt-[calc(env(safe-area-inset-top)+16px)] pr-4 pb-4 pl-4">
          <Text class="mb-4 text-xl font-bold">Gestures</Text>

          <Text class="mb-2 text-base font-semibold">Pan (drag)</Text>
          <Text testID="pan-end" class="mb-2 text-xs text-[#64748b]">
            Translation: ({panEndX()}, {panEndY()})
          </Text>
          <View
            testID="gesture-card"
            pan={{ x: translateX, y: translateY }}
            onPanEnd={(e) => {
              panEndX.set(Math.round(e.translation.x));
              panEndY.set(Math.round(e.translation.y));
              translateX.spring(0, { velocity: e.velocity.x });
              translateY.spring(0, { velocity: e.velocity.y });
            }}
            class="mb-6 flex h-30 w-50 flex-col items-center justify-center rounded-xl bg-[#3b82f6]"
            style={{ transform: [{ translateX }, { translateY }] }}
          >
            <Text class="text-sm font-semibold text-white">Drag Me</Text>
          </View>

          <Text class="mb-2 text-base font-semibold">Pinch (scale)</Text>
          <Text testID="pinch-end" class="mb-2 text-xs text-[#64748b]">
            Scale: {pinchEndScale().toFixed(2)}
          </Text>
          <View
            testID="pinch-card"
            pinch={{ scale }}
            onPinchEnd={(e) => {
              pinchEndScale.set(e.scale);
              scale.spring(1, { velocity: e.velocity });
            }}
            class="mb-6 flex h-30 w-50 flex-col items-center justify-center rounded-xl bg-[#8b5cf6]"
            style={{ transform: [{ scale }] }}
          >
            <Text class="text-sm font-semibold text-white">Pinch Me</Text>
            <Text class="text-[10px] text-indigo-100">Two fingers</Text>
          </View>

          <Text class="mb-2 text-base font-semibold">Rotation (angle)</Text>
          <Text testID="rotation-end" class="mb-2 text-xs text-[#64748b]">
            Angle: {rotationEndAngle().toFixed(0)}°
          </Text>
          <View
            testID="rotation-card"
            rotation={{ angle }}
            onRotationEnd={(e) => {
              rotationEndAngle.set(e.angle);
              angle.spring(0, { velocity: e.velocity });
            }}
            class="mb-6 flex h-30 w-50 flex-col items-center justify-center rounded-xl bg-[#ec4899]"
            style={{ transform: [{ rotate: angle }] }}
          >
            <Text class="text-sm font-semibold text-white">Rotate Me</Text>
            <Text class="text-[10px] text-pink-100">Two fingers</Text>
          </View>

          <Text class="mb-2 text-base font-semibold">Horizontal pan in vertical scroll</Text>
          <Text class="mb-2 text-xs text-[#64748b]">
            Swipe left/right on the card (scroll negotiation)
          </Text>
          <View
            testID="scroll-pan-card"
            pan={{ x: translateX }}
            onPanEnd={(e) => {
              translateX.spring(0, { velocity: e.velocity.x });
            }}
            class="mb-6 flex h-20 w-50 flex-col items-center justify-center rounded-xl bg-[#10b981]"
            style={{ transform: [{ translateX }] }}
          >
            <Text class="text-sm font-semibold text-white">Swipe Horizontal</Text>
          </View>

          <Text class="mt-4 text-xs text-[#64748b]">
            Note: Pinch and rotation require two fingers on real devices.{"\n"}
            Headless tests only cover pan (drag). Manual testing required for pinch/rotation.
          </Text>
        </View>
      </Scroll>
    </NavPage>
  );
}
