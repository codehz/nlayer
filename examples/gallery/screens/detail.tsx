import { Scroll, Text, View } from "@nlayer/ui";
import { detailBackIntercepted, detailInterceptBack } from "../nav";

export function DetailPage() {
  return (
    <Scroll testID="detail" class="flex-1 bg-white">
      <View class="p-4">
        <Text testID="detail-title" class="mb-3 text-2xl font-bold">
          Detail Screen
        </Text>
        <Text class="mb-4">This demonstrates push/pop navigation with native header</Text>
        <Text
          testID="intercept-back"
          onPress={() => detailInterceptBack.set(!detailInterceptBack())}
          class="mb-4 rounded-md bg-[#cbd5e1] p-3 text-center"
          classList={{ "bg-[#10b981]": detailInterceptBack() }}
        >
          {detailInterceptBack() ? "Back Interception ON" : "Enable Back Interception"}
        </Text>
        {detailBackIntercepted() && (
          <Text testID="back-intercepted" class="font-semibold text-[#10b981]">
            Back intercepted!
          </Text>
        )}
      </View>
    </Scroll>
  );
}
