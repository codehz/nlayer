import { signal } from "@nlayer/runtime";
import { Scroll, Text, View } from "@nlayer/ui";

export function HeaderDemoPage() {
  const transparent = signal(false);

  return (
    <Scroll
      testID="header-demo"
      class="flex-1 bg-white"
      classList={{ "bg-[#3b82f6]": transparent() }}
    >
      <View class="p-4" classList={{ "pt-30": transparent() }}>
        <Text
          class="mb-3 text-2xl font-bold text-black"
          classList={{ "text-white": transparent() }}
        >
          Header Demo
        </Text>
        <Text class="mb-4 text-[#64748b]" classList={{ "text-sky-50": transparent() }}>
          iOS/Android: native header. Web: app-drawn header slot.
        </Text>
        <Text
          testID="toggle-transparent"
          onPress={() => transparent.set(!transparent())}
          class="mb-4 rounded-md bg-[#3b82f6] p-3 text-center text-white"
          classList={{ "bg-[#1e40af]": transparent() }}
        >
          {transparent() ? "Transparent ON" : "Toggle Transparent"}
        </Text>
      </View>
    </Scroll>
  );
}
