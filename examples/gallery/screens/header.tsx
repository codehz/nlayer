import { Scroll, Text, View } from "@nlayer/ui";
import { headerTransparent } from "../nav";

export function HeaderDemoPage() {
  return (
    <Scroll
      testID="header-demo"
      class="flex-1 bg-white"
      classList={{ "bg-[#3b82f6]": headerTransparent() }}
    >
      <View class="p-4 pt-[calc(env(safe-area-inset-top)+16px)]">
        <Text
          class="mb-3 text-2xl font-bold text-black"
          classList={{ "text-white": headerTransparent() }}
        >
          Header Demo
        </Text>
        <Text class="mb-4 text-[#64748b]" classList={{ "text-sky-50": headerTransparent() }}>
          The bar matches the phone: back, title, right. Transparent lets this page show through.
        </Text>
        <Text
          testID="toggle-transparent"
          onPress={() => headerTransparent.set(!headerTransparent())}
          class="mb-4 rounded-md bg-[#3b82f6] p-3 text-center text-white"
          classList={{ "bg-[#1e40af]": headerTransparent() }}
        >
          {headerTransparent() ? "Transparent ON" : "Toggle Transparent"}
        </Text>
      </View>
    </Scroll>
  );
}
