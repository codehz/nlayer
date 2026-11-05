import { cleanup, effect, resource, signal, storage } from "@nlayer/runtime";
import {
  BackHandler,
  Image,
  Input,
  List,
  Scroll,
  Text,
  View,
  createAnimatedValue,
  platform,
} from "@nlayer/ui";
import mark from "../mark.png";
import { dismissedCount, nav } from "../nav";
import { quoteUrl } from "../quote";
import { Section } from "../Section";

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
        onChange={({ value }) => focused.set(value)}
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
  onChange?: (e: { value: string }) => void;
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
        onChange={({ value }) => name.set(value)}
      />
      <Field
        label="Email"
        value={email()}
        placeholder="Email"
        keyboardType="email"
        onChange={({ value }) => email.set(value)}
      />
      <Field
        label="Amount"
        value={amount()}
        placeholder="Amount"
        keyboardType="number"
        onChange={({ value }) => amount.set(value)}
      />
      <Field
        label="Phone"
        value={phone()}
        placeholder="Phone"
        keyboardType="phone"
        onChange={({ value }) => phone.set(value)}
      />
      <Field
        label="Password"
        value={secret()}
        placeholder="Password"
        secureTextEntry
        onChange={({ value }) => secret.set(value)}
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
        onChange={({ value }) => inputValue.set(value)}
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

export function HomePage() {
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
