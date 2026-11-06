import { signal } from "@nlayer/runtime";
import { NavPage, Scroll, Text, View } from "@nlayer/ui";
import { dismiss } from "../nav";
import { Section } from "../Section";

/**
 * A green layer under a slate box. The box's `pointerEvents` decides whether
 * a press on the box, on its button, or on the green strip lands where.
 */
function Stack(props: {
  name: string;
  note: string;
  pointer?: "none" | "box-none" | "box-only";
  /** Class on the slate box. A style `pointerEvents` still wins over this. */
  boxClass?: string;
  /** Class on the button. `pointer-events-auto` punches through a class `none`. */
  buttonClass?: string;
}) {
  const hit = signal("—");
  return (
    <View class="flex flex-col gap-1.5">
      <Text class="text-xs text-[#64748b]">{props.note}</Text>
      <View class="relative h-28">
        <View
          onPress={() => hit.set("垫层")}
          class="absolute inset-0 justify-end rounded-lg bg-[#d1fae5] p-2"
        >
          <Text class="text-xs text-[#065f46]">垫层</Text>
        </View>
        <View
          onPress={() => hit.set("盒")}
          style={props.pointer ? { pointerEvents: props.pointer } : undefined}
          class={`absolute inset-x-3 top-2 flex h-16 flex-row items-center justify-between rounded-lg bg-[#e2e8f0] p-2 ${props.boxClass ?? ""}`}
        >
          <Text class="text-xs text-[#334155]">{props.name}</Text>
          <View
            onPress={() => hit.set("按钮")}
            class={`rounded bg-[#0284c7] px-2 py-1 ${props.buttonClass ?? ""}`}
          >
            <Text class="text-xs text-white">按钮</Text>
          </View>
        </View>
      </View>
      <Text class="text-sm text-[#0f172a]">{`${props.name} → ${hit()}`}</Text>
    </View>
  );
}

/** A `none` banner over a scroll. Dragging the banner still scrolls. */
function ScrollUnderNone() {
  const hit = signal("—");
  return (
    <View class="flex flex-col gap-1.5">
      <Text class="text-xs text-[#64748b]">
        罩层是 none，盖住上面两行。点罩层落到那一行；拖罩层仍滚动下面的列表。
      </Text>
      <View class="relative h-36">
        <Scroll class="h-36 rounded-lg bg-white">
          <View class="flex flex-col">
            {["一", "二", "三", "四", "五", "六"].map((row) => (
              <View onPress={() => hit.set(row)} class="border-b border-[#e2e8f0] p-3">
                <Text class="text-sm text-[#0f172a]">{`行 ${row}`}</Text>
              </View>
            ))}
          </View>
        </Scroll>
        <View
          style={{ pointerEvents: "none" }}
          class="absolute inset-x-0 top-0 flex h-16 items-center justify-center bg-[#fecaca]/80"
        >
          <Text class="text-xs text-[#991b1b]">罩层 none</Text>
        </View>
      </View>
      <Text class="text-sm text-[#0f172a]">{`滚动 → ${hit()}`}</Text>
    </View>
  );
}

export function PointerEventsPage() {
  return (
    <NavPage title="Pointer events" onDismiss={dismiss}>
      <Scroll testID="pointer-events-page" class="flex-1 bg-slate-50">
        <View class="pt-[calc(env(safe-area-inset-top)+16px)] pr-[calc(env(safe-area-inset-right)+16px)] pb-[calc(max(env(safe-area-inset-bottom),env(keyboard-inset-height))+32px)] pl-[calc(env(safe-area-inset-left)+16px)]">
          <Text class="text-[28px] font-bold text-[#0f172a]">Pointer events</Text>
          <Text class="text-sm text-[#64748b]">
            四值挂在宿主上。类 pointer-events-none 里若有 pointer-events-auto，none 先降成
            box-none。
          </Text>

          <Section title="四值">
            <Stack name="auto" note="盒和按钮都接。绿边是露出来的垫层。" />
            <Stack name="none" note="盒和按钮都不接，点哪里都是垫层。" pointer="none" />
            <Stack
              name="box-none"
              note="按钮接。点盒的空白落到垫层，盒自己不接。"
              pointer="box-none"
            />
            <Stack name="box-only" note="只有盒接。点按钮也是盒，按钮不接。" pointer="box-only" />
          </Section>

          <Section title="类">
            <Stack
              name="类 none"
              note="pointer-events-none 里没有 auto，整棵都不接。"
              boxClass="pointer-events-none"
            />
            <Stack
              name="类 auto"
              note="none 里有 pointer-events-auto：盒降成 box-none，按钮仍接，空白落到垫层。"
              boxClass="pointer-events-none"
              buttonClass="pointer-events-auto"
            />
            <Stack
              name="样式盖过类"
              note="style pointerEvents none 盖过按钮上的 pointer-events-auto，整棵都不接。"
              pointer="none"
              buttonClass="pointer-events-auto"
            />
          </Section>

          <Section title="滚动">
            <ScrollUnderNone />
            <Text class="text-xs text-[#64748b]">
              下面这个滚动容器自己是 box-none：点行能点到，拖空白不滚动。
            </Text>
            <Scroll
              style={{ pointerEvents: "box-none" }}
              class="h-24 rounded-lg border border-[#e2e8f0] bg-white"
            >
              <View class="h-10 justify-center p-3">
                <Text class="text-sm text-[#0f172a]">这行接</Text>
              </View>
              <View class="h-24" />
            </Scroll>
          </Section>
        </View>
      </Scroll>
    </NavPage>
  );
}
