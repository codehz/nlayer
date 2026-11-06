import { signal } from "@nlayer/runtime";
import { Dialog, NavPage, Text, View } from "@nlayer/ui";
import { dismiss } from "../nav";

export function DialogDemoPage() {
  const dialogOpen = signal(false);
  const dialogAnimation = signal<"fade" | "slide" | "none">("fade");
  const lastCloseReason = signal<string>("");
  const closeOnBackdrop = signal(true);
  const closeOnEscape = signal(true);

  return (
    <NavPage title="Dialog" onDismiss={dismiss}>
      <View
        testID="dialog-page"
        class="flex-1 bg-white pt-[calc(env(safe-area-inset-top)+16px)] pr-4 pb-4 pl-4"
      >
        <Text class="mb-4 text-xl font-bold">Dialog Demo</Text>

        {lastCloseReason() && (
          <Text testID="close-reason" class="mb-3 font-semibold text-[#10b981]">
            Closed by: {lastCloseReason()}
          </Text>
        )}

        <View class="mb-4 flex flex-col gap-3">
          <Text
            testID="open-dialog-fade"
            onPress={() => {
              dialogAnimation.set("fade");
              dialogOpen.set(true);
            }}
            class="rounded-md bg-[#3b82f6] p-3 text-center text-white"
          >
            Open Dialog (fade)
          </Text>
          <Text
            testID="open-slide"
            onPress={() => {
              dialogAnimation.set("slide");
              dialogOpen.set(true);
            }}
            class="rounded-md bg-[#8b5cf6] p-3 text-center text-white"
          >
            Open Dialog (slide)
          </Text>
          <Text
            testID="open-none"
            onPress={() => {
              dialogAnimation.set("none");
              dialogOpen.set(true);
            }}
            class="rounded-md bg-[#ec4899] p-3 text-center text-white"
          >
            Open Dialog (none)
          </Text>
        </View>

        <View class="mb-4 flex flex-col gap-2">
          <Text
            testID="toggle-backdrop"
            onPress={() => closeOnBackdrop.set(!closeOnBackdrop())}
            class="rounded bg-[#cbd5e1] p-2"
            classList={{ "bg-[#10b981]": closeOnBackdrop() }}
          >
            closeOnBackdropPress: {closeOnBackdrop() ? "true" : "false"}
          </Text>
          <Text
            testID="toggle-escape"
            onPress={() => closeOnEscape.set(!closeOnEscape())}
            class="rounded bg-[#cbd5e1] p-2"
            classList={{ "bg-[#10b981]": closeOnEscape() }}
          >
            closeOnEscape: {closeOnEscape() ? "true" : "false"}
          </Text>
        </View>

        {dialogOpen() && (
          <Dialog
            testID="demo-dialog"
            animation={dialogAnimation()}
            closeOnBackdropPress={closeOnBackdrop()}
            closeOnEscape={closeOnEscape()}
            backdropStyle={{ backgroundColor: "#00000066" }}
            class="flex-1 items-center justify-center"
            onRequestClose={(e) => {
              lastCloseReason.set(e.reason);
            }}
            onOpenChange={(e) => {
              if (!e.open) {
                dialogOpen.set(false);
              }
            }}
          >
            <View class="min-w-70 rounded-xl bg-white p-6">
              <Text class="mb-3 text-lg font-bold">Dialog Title</Text>
              <Text class="mb-4 text-[#64748b]">
                Press back() to close with reason "cancel", or press Close button
              </Text>
              <Text
                testID="close-dialog"
                onPress={() => dialogOpen.set(false)}
                class="rounded-md bg-[#3b82f6] p-3 text-center text-white"
              >
                Close
              </Text>
            </View>
          </Dialog>
        )}
      </View>
    </NavPage>
  );
}
