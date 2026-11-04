import { cleanup, effect, mount, resource, signal, storage } from "@nlayer/runtime";
import {
  Animated,
  BackHandler,
  Dialog,
  Image,
  Input,
  List,
  NavPage,
  NavStack,
  Scroll,
  Text,
  View,
  createAnimatedValue,
  createNavStack,
  platform,
} from "@nlayer/ui";
import mark from "./mark.png";

declare global {
  var __nlayerTestOrigin: string | undefined;
  var __nlayerOs: string | undefined;
}

/** Headless tests serve the app directory here. The browser fetches the same file from the page. */
function quoteUrl(): string {
  const origin = globalThis.__nlayerTestOrigin;
  if (typeof origin === "string" && origin !== "") return `${origin}/quote.json`;
  return "/quote.json";
}

const nav = createNavStack<Route>([{ name: "home" }]);
type NavStackHandle = typeof nav;
const detailInterceptBack = signal(false);
const detailBackIntercepted = signal(false);
const dismissedCount = signal(0);

function Section(props: { title: string; children?: unknown }) {
  return (
    <View class="flex flex-col gap-2 pt-5">
      <Text class="text-xs font-semibold text-[#64748b]">{props.title}</Text>
      <View class="flex flex-col gap-2.5 rounded-xl border border-slate-200 bg-white p-3">
        {props.children}
      </View>
    </View>
  );
}

function Layout() {
  const tapped = signal(false);
  return (
    <Section title="View">
      <View class="flex w-40 flex-col gap-2">
        <View class="h-7 rounded-md bg-blue-100 p-1">
          <Text>One</Text>
        </View>
        <View class="h-7 rounded-md bg-blue-100 p-1">
          <Text>Two</Text>
        </View>
        <View class="h-7 rounded-md bg-blue-100 p-1">
          <Text>Three</Text>
        </View>
      </View>
      <View class="flex flex-row gap-2">
        <View class="h-7 w-16 rounded-md bg-slate-200 p-1">
          <Text>Left</Text>
        </View>
        <View class="h-7 w-16 rounded-md bg-slate-200 p-1">
          <Text>Right</Text>
        </View>
      </View>
      <View class="grid w-45 grid-cols-2 grid-rows-[32px_32px] gap-2">
        <View class="rounded-md bg-slate-100 p-1.5">
          <Text>A</Text>
        </View>
        <View class="rounded-md bg-slate-100 p-1.5">
          <Text>B</Text>
        </View>
        <View class="rounded-md bg-slate-100 p-1.5">
          <Text>C</Text>
        </View>
        <View class="rounded-md bg-slate-100 p-1.5">
          <Text>D</Text>
        </View>
      </View>
      <View onPress={() => tapped.set(true)} class="self-start rounded-lg bg-[#0f172a] p-2">
        <Text class="text-white">{tapped() ? "Pressed" : "Press"}</Text>
      </View>
      <View class="h-9 w-18 translate-x-3 rotate-15 scale-125 rounded-md bg-[#fde68a] p-1.5">
        <Text>Shifted</Text>
      </View>
      <View class="rounded-md bg-cyan-50 pt-[env(safe-area-inset-top)] pl-[env(safe-area-inset-left)]">
        <Text>Inset</Text>
      </View>
    </Section>
  );
}

function Pointer() {
  const faded = signal(false);
  const lit = signal(false);
  const focused = signal("");
  return (
    <Section title="Pointer">
      <View
        onPress={() => faded.set(true)}
        activeStyle={{ opacity: 0.4, width: 10 }}
        class="h-9 w-40 justify-center rounded-lg bg-[#0f172a] p-2"
      >
        <Text class="text-white">{faded() ? "Faded" : "Fade"}</Text>
      </View>
      <Text class="text-xs text-[#64748b]">{"activeStyle={{ opacity: 0.4 }}"}</Text>
      <View
        onPress={() => lit.set(true)}
        activeStyle={{ backgroundColor: "#bae6fd" }}
        class="self-start rounded-lg bg-slate-200 p-2"
      >
        <Text>{lit() ? "Lit" : "Light"}</Text>
      </View>
      <Text class="text-xs text-[#64748b]">{"activeStyle={{ backgroundColor }}"}</Text>
      <View
        hoverStyle={{ backgroundColor: "#dbeafe", width: 10 }}
        class="h-9 w-30 justify-center rounded-lg bg-slate-100 p-2"
      >
        <Text>Hover</Text>
      </View>
      <Text class="text-xs text-[#64748b]">{"hoverStyle={{ backgroundColor }}"}</Text>
      <Input
        value={focused()}
        placeholder="Focus"
        onChange={(value) => focused.set(value)}
        focusStyle={{ backgroundColor: "#ecfeff", color: "#0f172a", width: 10 }}
        class="w-40 rounded-lg border border-[#cbd5e1] bg-white p-2 text-base text-[#0f172a]"
      />
      <Text class="text-xs text-[#64748b]">{"focusStyle={{ backgroundColor }}"}</Text>
    </Section>
  );
}

function Type() {
  return (
    <Section title="Text">
      <Text class="text-base text-[#0f172a]">
        Plain
        <Text class="font-bold"> bold</Text>
        <Text class="italic"> italic</Text>
        <Text class="text-[22px] text-[#c2410c]"> color</Text>
      </Text>
    </Section>
  );
}

function Pictures() {
  return (
    <Section title="Image">
      <Image src={mark} class="size-8 rounded-md" />
      <Image src="https://example.com/photo.jpg" class="h-8 w-12 rounded-md object-contain" />
    </Section>
  );
}

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

function Field(props: {
  label: string;
  value: string;
  placeholder: string;
  onChange?: (value: string) => void;
  secureTextEntry?: boolean;
  keyboardType?: "text" | "email" | "number" | "phone";
  disabled?: boolean;
}) {
  return (
    <View class="flex flex-col gap-1">
      <Text class="text-xs text-[#64748b]">{props.label}</Text>
      <Input
        value={props.value}
        placeholder={props.placeholder}
        onChange={props.onChange}
        secureTextEntry={props.secureTextEntry}
        keyboardType={props.keyboardType}
        disabled={props.disabled}
        class="rounded-lg border border-[#cbd5e1] bg-white p-2 text-base text-[#0f172a]"
        classList={{ "bg-slate-100": props.disabled }}
      />
    </View>
  );
}

function Fields() {
  const name = signal("");
  const email = signal("");
  const amount = signal("");
  const phone = signal("");
  const secret = signal("");
  const locked = signal("locked");
  return (
    <Section title="Input">
      <Field
        label={`Name: ${name()}`}
        value={name()}
        placeholder="Name"
        onChange={(value) => name.set(value)}
      />
      <Field
        label="Email"
        value={email()}
        placeholder="Email"
        keyboardType="email"
        onChange={(value) => email.set(value)}
      />
      <Field
        label="Amount"
        value={amount()}
        placeholder="Amount"
        keyboardType="number"
        onChange={(value) => amount.set(value)}
      />
      <Field
        label="Phone"
        value={phone()}
        placeholder="Phone"
        keyboardType="phone"
        onChange={(value) => phone.set(value)}
      />
      <Field
        label="Password"
        value={secret()}
        placeholder="Password"
        secureTextEntry
        onChange={(value) => secret.set(value)}
      />
      <Field label={`Locked: ${locked()}`} value={locked()} placeholder="Locked" disabled />
    </Section>
  );
}

function Quote() {
  const status = signal("Loading quote");
  effect(() => {
    const controller = new AbortController();
    fetch(quoteUrl(), { signal: controller.signal })
      .then((response) => {
        if (!response.ok) throw new Error(String(response.status));
        return response.json();
      })
      .then((body) => {
        const quote = body as { title: string; count: number };
        status.set(`${quote.title}: ${quote.count}`);
      })
      .catch(() => status.set("Quote failed"));
    cleanup(() => controller.abort());
  });
  return (
    <Section title="Fetch">
      <Text>{status()}</Text>
    </Section>
  );
}

function Surface() {
  return (
    <Section title="Platform">
      <View class="flex flex-row items-center justify-between">
        <Text class="text-xs text-[#64748b]">platform.os</Text>
        <Text>{platform.os}</Text>
      </View>
    </Section>
  );
}

function VirtualList() {
  const items = Array.from({ length: 5000 }, (_, i) => ({ id: i, title: `Item ${i}` }));
  const count = signal(items.length);
  let listRef:
    | {
        scrollToIndex: (index: number, options?: { animated?: boolean }) => void;
        scrollToEnd: (options?: { animated?: boolean }) => void;
      }
    | undefined;

  return (
    <Section title="List">
      <Text class="mb-2 text-sm text-[#64748b]">
        {count()} items, recycled. Scroll to see more.
      </Text>
      <View class="h-75 rounded-lg border border-slate-200">
        <List
          each={items.slice(0, count())}
          estimatedItemSize={44}
          header={
            <View class="bg-slate-100 p-3">
              <Text class="font-semibold">Header</Text>
            </View>
          }
          footer={
            <View class="bg-slate-100 p-3">
              <Text class="font-semibold">Footer</Text>
            </View>
          }
          separator={<View class="h-px bg-slate-200" />}
          onLoadMore={() => {
            if (count() < items.length) {
              count.set(Math.min(count() + 50, items.length));
            }
          }}
          ref={(handle) => (listRef = handle)}
          class="flex-1"
        >
          {(item, index) => (
            <View class="p-3">
              <Text class="text-sm text-[#0f172a]">
                {item().title} (index: {index()})
              </Text>
            </View>
          )}
        </List>
      </View>
      <View class="mt-2 flex flex-row gap-2">
        <View
          testID="scroll-to-end"
          class="rounded-md bg-[#3b82f6] p-2"
          onPress={() => listRef?.scrollToEnd({ animated: true })}
        >
          <Text class="text-xs text-white">Scroll to End</Text>
        </View>
        <View
          testID="scroll-to-top"
          class="rounded-md bg-[#3b82f6] p-2"
          onPress={() => listRef?.scrollToIndex(0, { animated: true })}
        >
          <Text class="text-xs text-white">Scroll to Top</Text>
        </View>
      </View>
    </Section>
  );
}

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

function DataFetch() {
  const trigger = signal(0);
  const data = resource(
    () => trigger(),
    async (_, { signal }) => {
      const response = await fetch(quoteUrl(), { signal });
      if (!response.ok) throw new Error("Failed to fetch");
      return response.json();
    },
  );

  return (
    <Section title="Resource">
      <Text class="mb-2 text-sm text-[#64748b]">
        State: {data.state()} {data.loading() && "(loading)"}
      </Text>
      {data.error() && <Text class="text-sm text-[#ef4444]">Error: {String(data.error())}</Text>}
      {data() && <Text class="text-sm text-[#0f172a]">{JSON.stringify(data(), null, 2)}</Text>}
      <View
        testID="refetch"
        class="mt-2 self-start rounded-md bg-[#3b82f6] p-2"
        onPress={() => data.refetch()}
      >
        <Text class="text-xs text-white">Refetch</Text>
      </View>
    </Section>
  );
}

function StorageDemo() {
  const storedValue = signal("");
  const inputValue = signal("");

  effect(() => {
    storage.getItem("demo-key").then((value) => {
      storedValue.set(value || "(empty)");
    });
  });

  const save = () => {
    storage.setItem("demo-key", inputValue()).then(() => {
      storedValue.set(inputValue());
    });
  };

  const clear = () => {
    storage.removeItem("demo-key").then(() => {
      storedValue.set("(empty)");
      inputValue.set("");
    });
  };

  return (
    <Section title="Storage">
      <Text class="mb-2 text-sm text-[#64748b]">Stored: {storedValue()}</Text>
      <Input
        testID="storage-input"
        value={inputValue()}
        onChange={inputValue.set}
        placeholder="Enter value to store"
        class="rounded-md border border-[#cbd5e1] p-2"
      />
      <View class="mt-2 flex flex-row gap-2">
        <View testID="save-storage" class="rounded-md bg-[#3b82f6] p-2" onPress={save}>
          <Text class="text-xs text-white">Save</Text>
        </View>
        <View testID="clear-storage" class="rounded-md bg-[#ef4444] p-2" onPress={clear}>
          <Text class="text-xs text-white">Clear</Text>
        </View>
      </View>
    </Section>
  );
}

function ColorScheme() {
  const scheme = signal(platform.colorScheme());

  effect(() => {
    scheme.set(platform.colorScheme());
  });

  return (
    <Section title="Color Scheme">
      <Text class="text-sm text-[#64748b]">
        Current scheme: {scheme()} (change system settings to see update)
      </Text>
      <View
        class="mt-2 rounded-lg bg-slate-50 p-4"
        classList={{ "bg-[#1e293b]": scheme() === "dark" }}
      >
        <Text class="text-[#0f172a]" classList={{ "text-slate-100": scheme() === "dark" }}>
          This box adapts to {scheme()} mode
        </Text>
      </View>
    </Section>
  );
}

function BackNav() {
  const handled = signal(false);

  effect(() => {
    const remove = BackHandler.addEventListener(() => {
      handled.set(true);
      setTimeout(() => handled.set(false), 1000);
      return true; // handled
    });
    cleanup(remove);
  });

  return (
    <Section title="Back Handler">
      <Text class="text-sm text-[#64748b]">
        Press back button (Android) or browser back to trigger.
      </Text>
      {handled() && <Text class="mt-2 text-sm text-[#10b981]">Back handled!</Text>}
    </Section>
  );
}

type Route =
  | { name: "home" }
  | { name: "detail"; id: number }
  | { name: "dialog" }
  | { name: "gesture" }
  | { name: "list" }
  | { name: "header" }
  | { name: "viewport-parent" }
  | { name: "reading" }
  | { name: "classes" }
  | { name: "effects" };
function DialogDemoPage(props: { nav: NavStackHandle }) {
  const dialogOpen = signal(false);
  const dialogAnimation = signal<"fade" | "slide" | "none">("fade");
  const lastCloseReason = signal<string>("");
  const closeOnBackdrop = signal(true);
  const closeOnEscape = signal(true);

  return (
    <View testID="dialog-page" class="flex-1 bg-white p-4">
      <Text class="mb-4 text-xl font-bold">Dialog Demo</Text>
      <Text testID="dialog-back" onPress={() => props.nav.pop()} class="mb-4 text-[#3b82f6]">
        ← Back
      </Text>

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
          onRequestClose={(e) => {
            lastCloseReason.set(e.reason);
          }}
          onOpenChange={(e) => {
            if (!e.open) {
              dialogOpen.set(false);
            }
          }}
        >
          <View class="min-w-70 rounded-xl bg-white pt-[calc(env(safe-area-inset-top)+24px)] pr-[calc(env(safe-area-inset-right)+24px)] pb-[calc(env(safe-area-inset-bottom)+24px)] pl-[calc(env(safe-area-inset-left)+24px)]">
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
  );
}

function GestureCardPage(props: { nav: NavStackHandle }) {
  const translateX = createAnimatedValue(0);
  const translateY = createAnimatedValue(0);
  const panEndX = signal(0);
  const panEndY = signal(0);

  const scale = createAnimatedValue(1);
  const pinchEndScale = signal(1);

  const angle = createAnimatedValue(0);
  const rotationEndAngle = signal(0);

  return (
    <Scroll testID="gesture-page" class="flex-1 bg-white">
      <View class="p-4">
        <Text class="mb-4 text-xl font-bold">Gestures</Text>
        <Text testID="gesture-back" onPress={() => props.nav.pop()} class="mb-4 text-[#3b82f6]">
          ← Back
        </Text>

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
  );
}

function DetailPage(props: { nav: NavStackHandle }) {
  return (
    <Scroll testID="detail" class="flex-1 bg-white">
      <View class="p-4">
        <Text testID="detail-title" class="mb-3 text-2xl font-bold">
          Detail Screen
        </Text>
        <Text class="mb-4">This demonstrates push/pop navigation with native header</Text>
        <Text
          testID="go-back"
          onPress={() => props.nav.pop()}
          class="mb-4 font-semibold text-[#3b82f6]"
        >
          ← Back
        </Text>
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

function HeaderDemoPage(props: { nav: NavStackHandle }) {
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
        <Text
          testID="header-back"
          onPress={() => props.nav.pop()}
          class="font-semibold text-[#3b82f6]"
          classList={{ "text-white": transparent() }}
        >
          ← Back
        </Text>
      </View>
    </Scroll>
  );
}

function ListDemoPage(props: { nav: NavStackHandle }) {
  const loaded = signal(1000);
  const refreshing = signal(false);
  const snapEnabled = signal(false);
  const stickyEnabled = signal(false);
  const invertedEnabled = signal(false);
  const viewableFirst = signal(0);
  const viewableLast = signal(0);

  const handleRefresh = () => {
    refreshing.set(true);
    setTimeout(() => {
      refreshing.set(false);
    }, 1000);
  };

  return (
    <View testID="list-page" class="flex-1 bg-white">
      <View class="border-b border-b-slate-200 p-4">
        <Text class="text-xl font-bold">Virtual List</Text>
        <Text
          testID="list-back"
          onPress={() => props.nav.pop()}
          class="my-2 font-semibold text-[#3b82f6]"
        >
          ← Back
        </Text>

        <View class="mt-2 flex flex-col gap-2">
          <Text
            testID="toggle-snap"
            onPress={() => snapEnabled.set(!snapEnabled())}
            class="rounded p-1.5 text-xs bg-[#cbd5e1]"
            classList={{ "bg-[#10b981]": snapEnabled() }}
          >
            snapToInterval: {snapEnabled() ? "64px" : "off"}
          </Text>
          <Text
            testID="toggle-sticky"
            onPress={() => stickyEnabled.set(!stickyEnabled())}
            class="rounded p-1.5 text-xs bg-[#cbd5e1]"
            classList={{ "bg-[#10b981]": stickyEnabled() }}
          >
            stickyHeaderIndices: {stickyEnabled() ? "[0, 10, 20]" : "off"}
          </Text>
          <Text
            testID="toggle-inverted"
            onPress={() => invertedEnabled.set(!invertedEnabled())}
            class="rounded p-1.5 text-xs bg-[#cbd5e1]"
            classList={{ "bg-[#10b981]": invertedEnabled() }}
          >
            inverted: {invertedEnabled() ? "true" : "false"}
          </Text>
          <Text testID="viewable-range" class="text-[11px] text-[#64748b]">
            Viewable: {viewableFirst()} - {viewableLast()}
          </Text>
        </View>
      </View>
      <List
        testID="list"
        class="flex-1"
        each={Array.from({ length: loaded() }, (_, i) => i)}
        estimatedItemSize={32}
        refreshing={refreshing()}
        onRefresh={handleRefresh}
        snapToInterval={snapEnabled() ? 64 : undefined}
        stickyHeaderIndices={stickyEnabled() ? [0, 10, 20] : undefined}
        inverted={invertedEnabled()}
        onViewableItemsChange={(e) => {
          viewableFirst.set(e.first);
          viewableLast.set(e.last);
        }}
        onLoadMore={() => {
          if (loaded() < 5000) {
            loaded.set(Math.min(5000, loaded() + 1000));
          }
        }}
      >
        {(index) => (
          <View
            testID={index() === 0 ? "first-item" : undefined}
            class="h-8 justify-center border-b border-b-slate-100 bg-white px-4"
            classList={{
              "bg-[#e0f2fe]": [0, 10, 20].includes(index()) && stickyEnabled(),
            }}
          >
            <Text testID={index() === 0 ? "first" : undefined}>
              Row {index()} {[0, 10, 20].includes(index()) && stickyEnabled() ? "(sticky)" : ""}
            </Text>
          </View>
        )}
      </List>
    </View>
  );
}

function ViewportParentPage(props: { nav: NavStackHandle }) {
  return (
    <View testID="viewport-parent-page" class="flex-1 min-h-0 bg-white">
      <View class="border-b border-b-slate-200 p-4">
        <Text class="text-xl font-bold">List viewport="parent"</Text>
        <Text
          testID="vp-back"
          onPress={() => props.nav.pop()}
          class="mt-2 font-semibold text-[#3b82f6]"
        >
          ← Back
        </Text>
        <Text class="mt-2 text-xs text-[#64748b]">
          List uses outer Scroll's viewport (no nested scroll)
        </Text>
      </View>
      <Scroll testID="outer-scroll" class="flex-1">
        <View class="p-4">
          <Text class="mb-2 text-sm font-semibold">Content before list</Text>
        </View>
        <List
          testID="vp-list"
          viewport="parent"
          each={Array.from({ length: 100 }, (_, i) => i)}
          estimatedItemSize={40}
        >
          {(index) => (
            <View
              testID={`vp-item-${index()}`}
              class="h-10 justify-center border-b border-b-slate-100 px-4"
            >
              <Text>Item {index()}</Text>
            </View>
          )}
        </List>
        <View class="p-4">
          <Text class="mt-2 text-sm font-semibold">Content after list</Text>
        </View>
      </Scroll>
    </View>
  );
}
function ReadingPage(props: { nav: NavStackHandle }) {
  const pressed = signal(0);
  return (
    <Scroll testID="reading-page" class="flex-1 bg-slate-50">
      <View class="pt-[calc(env(safe-area-inset-top)+16px)] pr-[calc(env(safe-area-inset-right)+16px)] pb-8 pl-[calc(env(safe-area-inset-left)+16px)]">
        <Text
          testID="reading-back"
          onPress={() => props.nav.pop()}
          class="mb-2 font-semibold text-[#3b82f6]"
        >
          ← Gallery
        </Text>
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

function ClassSample(props: { name: string; code: string; children?: unknown }) {
  return (
    <View class="flex flex-1 flex-col gap-1.5">
      <Text class="text-sm font-semibold text-[#0f172a]">{props.name}</Text>
      <Text class="text-xs text-[#64748b]">{props.code}</Text>
      {props.children}
    </View>
  );
}

function ClassesPage(props: { nav: NavStackHandle }) {
  return (
    <Scroll testID="classes-page" class="flex-1 bg-slate-50">
      <View class="pt-[calc(env(safe-area-inset-top)+16px)] pr-[calc(env(safe-area-inset-right)+16px)] pb-[calc(max(env(safe-area-inset-bottom),env(keyboard-inset-height))+32px)] pl-[calc(env(safe-area-inset-left)+16px)]">
        <Text onPress={() => props.nav.pop()} class="mb-2 font-semibold text-[#3b82f6]">
          ← Gallery
        </Text>
        <Text class="text-[28px] font-bold text-[#0f172a]">Classes</Text>
        <Text class="text-sm text-[#64748b]">
          v5: layout, color, state, and names that only warn
        </Text>

        <Section title="State">
          <View class="flex flex-row gap-3">
            <ClassSample name="Opacity" code="opacity-50">
              <View class="flex flex-row items-center gap-2 self-start rounded-lg bg-[#0f172a] p-3">
                <View class="size-8 rounded-md bg-white" />
                <View testID="class-faded" class="size-8 rounded-md bg-white opacity-50" />
              </View>
            </ClassSample>
            <ClassSample name="Translate" code="translate-x-1">
              <View class="self-start rounded-lg bg-slate-100 p-3">
                <View class="flex flex-row items-start">
                  <View class="size-5 rounded bg-slate-300" />
                  <View
                    testID="class-shift"
                    class="-ml-5 size-5 translate-x-1 rounded bg-[#0f172a]"
                  />
                </View>
              </View>
            </ClassSample>
          </View>
          <View class="flex flex-row gap-3">
            <ClassSample name="Hover" code="hover:p-4">
              <View testID="class-hover" class="self-start rounded-lg bg-[#dbeafe] hover:p-4">
                <View testID="hover-child" class="rounded-lg bg-[#0f172a] px-3 py-2">
                  <Text class="text-xs font-semibold text-white">Hover</Text>
                </View>
              </View>
            </ClassSample>
            <ClassSample name="Press" code="active:scale-95">
              <View
                testID="class-press"
                onPress={() => {}}
                class="flex size-16 items-center justify-center self-start rounded-2xl bg-[#0f172a] active:scale-95"
              >
                <Text class="text-xs font-semibold text-white">Press</Text>
              </View>
            </ClassSample>
          </View>
          <ClassSample name="Focus" code="focus-within:p-4 focus-visible:opacity-50">
            <View testID="class-within" class="rounded-lg bg-slate-100 focus-within:p-4">
              <Input
                testID="class-field"
                value=""
                placeholder="Focus"
                onChange={() => {}}
                class="rounded-lg border border-[#cbd5e1] bg-white p-2 text-base text-[#0f172a] focus:p-2 focus-visible:opacity-50"
              />
            </View>
          </ClassSample>
          <ClassSample name="Disabled" code="disabled:opacity-25 enabled:opacity-50">
            <View class="flex flex-row gap-2">
              <View class="flex flex-1 flex-col items-center gap-2 rounded-lg bg-slate-100 py-4">
                <View
                  testID="class-disabled"
                  disabled
                  class="size-12 rounded-xl bg-[#0f172a] disabled:opacity-25 enabled:opacity-50"
                />
                <Text class="text-xs text-[#64748b]">disabled</Text>
              </View>
              <View class="flex flex-1 flex-col items-center gap-2 rounded-lg bg-slate-100 py-4">
                <View
                  testID="class-enabled"
                  class="size-12 rounded-xl bg-[#0f172a] disabled:opacity-25 enabled:opacity-50"
                />
                <Text class="text-xs text-[#64748b]">enabled</Text>
              </View>
            </View>
          </ClassSample>
        </Section>

        <Section title="Layout">
          <ClassSample name="Flex" code="flex-row items-start gap-3 p-4">
            <View
              testID="class-row"
              class="flex flex-row items-start gap-3 self-start rounded-lg bg-zinc-800 p-4"
            >
              <View testID="class-a" class="size-2.5 rounded-full bg-white" />
              <View testID="class-b" class="size-2.5 rounded-full bg-white" />
            </View>
          </ClassSample>
          <View class="flex flex-row gap-3">
            <ClassSample name="Space" code="space-x-2">
              <View class="self-start rounded-lg bg-slate-100 p-3">
                <View testID="class-space" class="flex flex-row items-start space-x-2">
                  <View testID="space-a" class="size-2 rounded-full bg-[#0f172a]" />
                  <View testID="space-b" class="size-2 rounded-full bg-[#0f172a]" />
                </View>
              </View>
            </ClassSample>
            <ClassSample name="Arbitrary" code="h-[11px] w-[37px]">
              <View class="self-start rounded-lg bg-slate-100 p-3">
                <View
                  testID="class-arbitrary"
                  class="h-[11px] w-[37px] rounded-full bg-[#7c3aed]"
                />
              </View>
            </ClassSample>
          </View>
        </Section>

        <Section title="Type">
          <View class="flex flex-row items-center gap-3">
            <Image testID="class-image" src={mark} class="size-16 rounded-lg object-cover" />
            <ClassSample name="Label" code="text-sm font-semibold text-zinc-800">
              <Text testID="class-label" class="text-sm font-semibold text-zinc-800">
                Cover mark
              </Text>
            </ClassSample>
          </View>
          <ClassSample name="Clamp" code="line-clamp-2">
            <View class="self-start rounded-lg bg-slate-50 p-3">
              <Text testID="class-clamp" class="w-40 text-sm leading-5 text-[#334155] line-clamp-2">
                one two three four five six seven eight nine ten
              </Text>
            </View>
          </ClassSample>
          <ClassSample name="Selection" code="selection:bg-zinc-200">
            <Text
              testID="class-select"
              class="text-base leading-6 text-[#0f172a] select-text selection:bg-zinc-200"
            >
              Select this sentence. The highlight is zinc-200.
            </Text>
          </ClassSample>
        </Section>

        <Section title="Precedence">
          <View class="flex flex-row gap-2">
            <ClassSample name="Important" code="!p-4 beats paddingTop">
              <View
                testID="class-important"
                class="flex flex-col rounded-lg bg-slate-100 !p-4"
                style={{ paddingTop: 1 }}
              >
                <View testID="important-child" class="h-8 rounded-md bg-[#0f172a]" />
              </View>
            </ClassSample>
            <ClassSample name="Style" code="paddingLeft replaces p-4">
              <View
                testID="class-override"
                class="flex flex-col rounded-lg bg-slate-100 p-4"
                style={{ paddingLeft: 8 }}
              >
                <View testID="override-child" class="h-8 rounded-md bg-[#7c3aed]" />
              </View>
            </ClassSample>
          </View>
          <ClassSample name="Warned" code="p-4 applies, blur-sm and wat do not">
            <View
              testID="class-mixed"
              class="flex flex-col gap-2 rounded-lg bg-amber-50 p-4 blur-sm wat"
            >
              <View testID="mixed-child" class="size-2 rounded-full bg-amber-800" />
              <Text class="text-xs leading-5 text-amber-900">
                Padding stays. blur-sm is unsupported and wat is unknown.
              </Text>
            </View>
          </ClassSample>
        </Section>
      </View>
    </Scroll>
  );
}

function EffectsPage(props: { nav: NavStackHandle }) {
  return (
    <Scroll testID="effects-page" class="flex-1 bg-slate-50">
      <View class="pt-[calc(env(safe-area-inset-top)+16px)] pr-[calc(env(safe-area-inset-right)+16px)] pb-[calc(max(env(safe-area-inset-bottom),env(keyboard-inset-height))+32px)] pl-[calc(env(safe-area-inset-left)+16px)]">
        <Text onPress={() => props.nav.pop()} class="mb-2 font-semibold text-[#3b82f6]">
          ← Gallery
        </Text>
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
  );
}

function HomePage() {
  return (
    <Scroll testID="page" class="flex-1 bg-slate-50">
      <View
        testID="safe"
        class="pt-[calc(env(safe-area-inset-top)+16px)] pr-[calc(env(safe-area-inset-right)+16px)] pb-[calc(max(env(safe-area-inset-bottom),env(keyboard-inset-height))+32px)] pl-[calc(env(safe-area-inset-left)+16px)]"
      >
        <Text testID="home-title" class="text-[28px] font-bold text-[#0f172a]">
          Gallery
        </Text>
        <Text class="text-sm text-[#64748b]">v3 Navigation: Stack, Screens, Modals, Dialogs</Text>
        <Surface />
        <Layout />
        <Pointer />
        <Type />
        <Pictures />
        <Scrollers />
        <ScrollEvents />
        <Fields />
        <Quote />
        <VirtualList />
        <Animations />
        <DataFetch />
        <StorageDemo />
        <ColorScheme />
        <BackNav />

        <Section title="Navigation Demos">
          <Text testID="dismissed-count" class="mb-2 text-xs text-[#64748b]">
            Dismissed: {dismissedCount()}
          </Text>
          <Text
            testID="open-detail"
            onPress={() => nav.push({ name: "detail", id: 1 })}
            class="p-2 font-semibold text-[#3b82f6]"
          >
            → Detail (push/pop/onDismiss/onBackPress)
          </Text>
          <Text
            testID="open-dialog"
            onPress={() => nav.push({ name: "dialog" })}
            class="p-2 font-semibold text-[#3b82f6]"
          >
            → Dialog Demo
          </Text>
          <Text
            testID="open-gesture"
            onPress={() => nav.push({ name: "gesture" })}
            class="p-2 font-semibold text-[#3b82f6]"
          >
            → Gesture Card (pan → translate)
          </Text>
          <Text
            testID="open-list"
            onPress={() => nav.push({ name: "list" })}
            class="p-2 font-semibold text-[#3b82f6]"
          >
            → List Demo (5000 rows)
          </Text>
          <Text
            testID="open-header"
            onPress={() => nav.push({ name: "header" })}
            class="p-2 font-semibold text-[#3b82f6]"
          >
            → Header Demo (title, headerStyle, headerTransparent)
          </Text>
          <Text
            testID="open-viewport-parent"
            onPress={() => nav.push({ name: "viewport-parent" })}
            class="p-2 font-semibold text-[#3b82f6]"
          >
            → List viewport="parent" (nested in Scroll)
          </Text>
          <Text
            testID="open-reading"
            onPress={() => nav.push({ name: "reading" })}
            class="p-2 font-semibold text-[#3b82f6]"
          >
            → Reading (selection, inline views, href)
          </Text>
          <Text
            testID="open-classes"
            onPress={() => nav.push({ name: "classes" })}
            class="p-2 font-semibold text-[#3b82f6]"
          >
            → Classes (utilities)
          </Text>
          <Text
            testID="open-effects"
            onPress={() => nav.push({ name: "effects" })}
            class="p-2 font-semibold text-[#3b82f6]"
          >
            → Effects (shadows, gradients)
          </Text>
        </Section>
      </View>
    </Scroll>
  );
}

function App() {
  return (
    <NavStack state={nav} class="flex-1">
      {(route, index) => {
        const r = route;
        let content;
        let title: string | undefined;
        let headerStyle:
          { backgroundColor?: string; color?: string; tintColor?: string } | undefined;
        let headerTransparent = false;
        let header: unknown;
        let headerLeft: unknown;
        let headerRight: unknown;

        if (r.name === "home") {
          content = <HomePage />;
          title = "Gallery";
        } else if (r.name === "detail") {
          content = <DetailPage nav={nav} />;
          title = "Detail";
          headerStyle = { backgroundColor: "#3b82f6", color: "#fff", tintColor: "#fff" };
        } else if (r.name === "dialog") {
          content = <DialogDemoPage nav={nav} />;
          title = "Dialog";
        } else if (r.name === "gesture") {
          content = <GestureCardPage nav={nav} />;
          title = "Gestures";
        } else if (r.name === "list") {
          content = <ListDemoPage nav={nav} />;
          title = "Virtual List";
        } else if (r.name === "header") {
          content = <HeaderDemoPage nav={nav} />;
          title = "Header";
          headerStyle = { backgroundColor: "#10b981", color: "#fff", tintColor: "#fff" };
          // iOS and Android put these in the system header; web draws
          // `header` at the top of the page instead.
          headerLeft = (
            <Text onPress={() => nav.pop()} class="font-semibold text-white">
              Back
            </Text>
          );
          headerRight = <Text class="text-white">Right</Text>;
          header = (
            <View
              testID="drawn-header"
              class="h-11 flex-row items-center justify-between bg-[#10b981] px-3"
            >
              <Text
                testID="drawn-header-back"
                onPress={() => nav.pop()}
                class="font-semibold text-white"
              >
                ← Back
              </Text>
              <Text class="font-bold text-white">Header</Text>
              <Text class="text-white">Right</Text>
            </View>
          );
        } else if (r.name === "viewport-parent") {
          content = <ViewportParentPage nav={nav} />;
          title = "Viewport Parent";
        } else if (r.name === "reading") {
          content = <ReadingPage nav={nav} />;
          title = "Reading";
        } else if (r.name === "classes") {
          content = <ClassesPage nav={nav} />;
          title = "Classes";
        } else if (r.name === "effects") {
          content = <EffectsPage nav={nav} />;
          title = "Effects";
        } else {
          content = <Text>Unknown route</Text>;
        }

        return (
          <NavPage
            title={title}
            headerStyle={headerStyle}
            headerTransparent={headerTransparent}
            header={header}
            headerLeft={headerLeft}
            headerRight={headerRight}
            onDismiss={() => dismissedCount.set(dismissedCount() + 1)}
            popOnBack={r.name !== "detail" || !detailInterceptBack()}
            onBackPress={
              r.name === "detail"
                ? () => {
                    if (detailInterceptBack()) {
                      detailBackIntercepted.set(true);
                      setTimeout(() => detailBackIntercepted.set(false), 1000);
                    }
                  }
                : undefined
            }
          >
            {content}
          </NavPage>
        );
      }}
    </NavStack>
  );
}

mount(App);
