import { signal } from "@nlayer/runtime";
import { NavPage, Scroll, Text, View } from "@nlayer/ui";
import { dismiss } from "../nav";
import { Section } from "../Section";

function Scrollers() {
  return (
    <Section title="Scroll">
      <Scroll class="h-20 w-50 rounded-lg bg-slate-50">
        <View class="h-50 w-50 p-2">
          <Text>Down</Text>
        </View>
      </Scroll>
      <Scroll testID="across" horizontal class="h-12 w-40 rounded-lg bg-slate-50">
        <View class="h-12 w-100 p-2">
          <Text>Across</Text>
        </View>
      </Scroll>
    </Section>
  );
}

function metric(event: {
  offset: { x: number; y: number };
  contentSize: { width: number; height: number };
  size: { width: number; height: number };
}): string {
  const n = (value: number) => String(Math.round(value));
  return `${n(event.offset.x)},${n(event.offset.y)} ${n(event.contentSize.width)}x${n(event.contentSize.height)} ${n(event.size.width)}x${n(event.size.height)}`;
}

function ScrollEvents() {
  const scroll = signal("");
  const scrollEnd = signal("");
  const dragStart = signal("");
  const dragEnd = signal("");
  const momentumStart = signal("");
  const momentumEnd = signal("");
  return (
    <Section title="Scroll events">
      <Text>{`onScroll ${scroll() || "none"}`}</Text>
      <Text>{`onScrollEnd ${scrollEnd() || "none"}`}</Text>
      <Text>{`onScrollDragStart ${dragStart() || "none"}`}</Text>
      <Text>{`onScrollDragEnd ${dragEnd() || "none"}`}</Text>
      <Text>{`onMomentumStart ${momentumStart() || "none"}`}</Text>
      <Text>{`onMomentumEnd ${momentumEnd() || "none"}`}</Text>
      <Scroll
        testID="events"
        onScroll={(event) => scroll.set(metric(event))}
        onScrollEnd={(event) => scrollEnd.set(metric(event))}
        onScrollDragStart={(event) => dragStart.set(metric(event))}
        onScrollDragEnd={(event) => dragEnd.set(metric(event))}
        onMomentumStart={(event) => momentumStart.set(metric(event))}
        onMomentumEnd={(event) => momentumEnd.set(metric(event))}
        class="h-20 w-50 rounded-lg bg-slate-50"
      >
        <View class="h-60 w-50 p-2">
          <Text>Events</Text>
        </View>
      </Scroll>
    </Section>
  );
}

export function ScrollPage() {
  return (
    <NavPage title="Scroll" onDismiss={dismiss}>
      <Scroll testID="scroll-page" class="flex-1 bg-slate-50">
        <View class="p-4">
          <Scrollers />
          <ScrollEvents />
        </View>
      </Scroll>
    </NavPage>
  );
}
