import { cleanup, effect, mount, resource, signal, storage } from "@nlayer/runtime";
import { Animated, BackHandler, Image, Input, List, Scroll, Text, View, createAnimatedValue, platform } from "@nlayer/ui";
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

function Section(props: { title: string; children?: unknown }) {
  return (
    <View style={{ gap: 8, paddingTop: 20 }}>
      <Text style={{ fontSize: 12, fontWeight: "600", color: "#64748b" }}>{props.title}</Text>
      <View
        style={{
          backgroundColor: "#ffffff",
          borderRadius: 12,
          padding: 12,
          gap: 10,
          borderWidth: 1,
          borderColor: "#e2e8f0",
        }}
      >
        {props.children}
      </View>
    </View>
  );
}

function Layout() {
  const tapped = signal(false);
  return (
    <Section title="View">
      <View style={{ width: 160, gap: 8 }}>
        <View style={{ height: 28, borderRadius: 6, backgroundColor: "#dbeafe", padding: 4 }}>
          <Text>One</Text>
        </View>
        <View style={{ height: 28, borderRadius: 6, backgroundColor: "#dbeafe", padding: 4 }}>
          <Text>Two</Text>
        </View>
        <View style={{ height: 28, borderRadius: 6, backgroundColor: "#dbeafe", padding: 4 }}>
          <Text>Three</Text>
        </View>
      </View>
      <View style={{ flexDirection: "row", gap: 8 }}>
        <View
          style={{ width: 64, height: 28, borderRadius: 6, backgroundColor: "#e2e8f0", padding: 4 }}
        >
          <Text>Left</Text>
        </View>
        <View
          style={{ width: 64, height: 28, borderRadius: 6, backgroundColor: "#e2e8f0", padding: 4 }}
        >
          <Text>Right</Text>
        </View>
      </View>
      <View
        style={{
          display: "grid",
          width: 180,
          gridTemplateColumns: "1fr 1fr",
          gridTemplateRows: "32px 32px",
          gap: 8,
        }}
      >
        <View style={{ backgroundColor: "#f1f5f9", borderRadius: 6, padding: 6 }}>
          <Text>A</Text>
        </View>
        <View style={{ backgroundColor: "#f1f5f9", borderRadius: 6, padding: 6 }}>
          <Text>B</Text>
        </View>
        <View style={{ backgroundColor: "#f1f5f9", borderRadius: 6, padding: 6 }}>
          <Text>C</Text>
        </View>
        <View style={{ backgroundColor: "#f1f5f9", borderRadius: 6, padding: 6 }}>
          <Text>D</Text>
        </View>
      </View>
      <View
        onPress={() => tapped.set(true)}
        style={{
          alignSelf: "flex-start",
          backgroundColor: "#0f172a",
          borderRadius: 8,
          padding: 8,
        }}
      >
        <Text style={{ color: "#ffffff" }}>{tapped() ? "Pressed" : "Press"}</Text>
      </View>
      <View
        style={{
          width: 72,
          height: 36,
          backgroundColor: "#fde68a",
          borderRadius: 6,
          padding: 6,
          transform: [{ translateX: 12 }, { rotate: "15deg" }, { scale: 1.25 }],
        }}
      >
        <Text>Shifted</Text>
      </View>
      <View
        style={{
          paddingTop: "env(safe-area-inset-top)",
          paddingLeft: "env(safe-area-inset-left)",
          backgroundColor: "#ecfeff",
          borderRadius: 6,
        }}
      >
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
        style={{
          width: 160,
          height: 36,
          justifyContent: "center",
          backgroundColor: "#0f172a",
          borderRadius: 8,
          padding: 8,
        }}
      >
        <Text style={{ color: "#ffffff" }}>{faded() ? "Faded" : "Fade"}</Text>
      </View>
      <Text style={{ fontSize: 12, color: "#64748b" }}>{"activeStyle={{ opacity: 0.4 }}"}</Text>
      <View
        onPress={() => lit.set(true)}
        activeStyle={{ backgroundColor: "#bae6fd" }}
        style={{
          alignSelf: "flex-start",
          backgroundColor: "#e2e8f0",
          borderRadius: 8,
          padding: 8,
        }}
      >
        <Text>{lit() ? "Lit" : "Light"}</Text>
      </View>
      <Text style={{ fontSize: 12, color: "#64748b" }}>{"activeStyle={{ backgroundColor }}"}</Text>
      <View
        hoverStyle={{ backgroundColor: "#dbeafe", width: 10 }}
        style={{
          width: 120,
          height: 36,
          justifyContent: "center",
          backgroundColor: "#f1f5f9",
          borderRadius: 8,
          padding: 8,
        }}
      >
        <Text>Hover</Text>
      </View>
      <Text style={{ fontSize: 12, color: "#64748b" }}>{"hoverStyle={{ backgroundColor }}"}</Text>
      <Input
        value={focused()}
        placeholder="Focus"
        onChange={(value) => focused.set(value)}
        focusStyle={{ backgroundColor: "#ecfeff", color: "#0f172a", width: 10 }}
        style={{
          width: 160,
          fontSize: 16,
          color: "#0f172a",
          backgroundColor: "#ffffff",
          borderWidth: 1,
          borderColor: "#cbd5e1",
          borderRadius: 8,
          padding: 8,
        }}
      />
      <Text style={{ fontSize: 12, color: "#64748b" }}>{"focusStyle={{ backgroundColor }}"}</Text>
    </Section>
  );
}

function Type() {
  return (
    <Section title="Text">
      <Text style={{ fontSize: 16, color: "#0f172a" }}>
        Plain
        <Text style={{ fontWeight: "700" }}> bold</Text>
        <Text style={{ fontStyle: "italic" }}> italic</Text>
        <Text style={{ fontSize: 22, color: "#c2410c" }}> color</Text>
      </Text>
    </Section>
  );
}

function Pictures() {
  return (
    <Section title="Image">
      <Image src={mark} style={{ width: 32, height: 32, borderRadius: 6 }} />
      <Image
        src="https://example.com/photo.jpg"
        style={{ width: 48, height: 32, borderRadius: 6, objectFit: "contain" }}
      />
    </Section>
  );
}

function Scrollers() {
  return (
    <Section title="Scroll">
      <Scroll style={{ height: 80, width: 200, backgroundColor: "#f8fafc", borderRadius: 8 }}>
        <View style={{ height: 200, width: 200, padding: 8 }}>
          <Text>Down</Text>
        </View>
      </Scroll>
      <Scroll
        testID="across"
        horizontal
        style={{ height: 48, width: 160, backgroundColor: "#f8fafc", borderRadius: 8 }}
      >
        <View style={{ width: 400, height: 48, padding: 8 }}>
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
        style={{ height: 80, width: 200, backgroundColor: "#f8fafc", borderRadius: 8 }}
      >
        <View style={{ height: 240, width: 200, padding: 8 }}>
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
    <View style={{ gap: 4 }}>
      <Text style={{ fontSize: 12, color: "#64748b" }}>{props.label}</Text>
      <Input
        value={props.value}
        placeholder={props.placeholder}
        onChange={props.onChange}
        secureTextEntry={props.secureTextEntry}
        keyboardType={props.keyboardType}
        disabled={props.disabled}
        style={{
          fontSize: 16,
          color: "#0f172a",
          backgroundColor: props.disabled ? "#f1f5f9" : "#ffffff",
          borderWidth: 1,
          borderColor: "#cbd5e1",
          borderRadius: 8,
          padding: 8,
        }}
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
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
        <Text style={{ fontSize: 12, color: "#64748b" }}>platform.os</Text>
        <Text>{platform.os}</Text>
      </View>
    </Section>
  );
}

function VirtualList() {
  const items = Array.from({ length: 5000 }, (_, i) => ({ id: i, title: `Item ${i}` }));
  const count = signal(items.length);
  let listRef: { scrollToIndex: (index: number, options?: { animated?: boolean }) => void; scrollToEnd: (options?: { animated?: boolean }) => void } | undefined;

  return (
    <Section title="List">
      <Text style={{ fontSize: 14, color: "#64748b", marginBottom: 8 }}>
        {count()} items, recycled. Scroll to see more.
      </Text>
      <View style={{ height: 300, borderWidth: 1, borderColor: "#e2e8f0", borderRadius: 8 }}>
        <List
          each={items.slice(0, count())}
          estimatedItemSize={44}
          header={<View style={{ padding: 12, backgroundColor: "#f1f5f9" }}><Text style={{ fontWeight: "600" }}>Header</Text></View>}
          footer={<View style={{ padding: 12, backgroundColor: "#f1f5f9" }}><Text style={{ fontWeight: "600" }}>Footer</Text></View>}
          separator={<View style={{ height: 1, backgroundColor: "#e2e8f0" }} />}
          onLoadMore={() => {
            if (count() < items.length) {
              count.set(Math.min(count() + 50, items.length));
            }
          }}
          ref={(handle) => (listRef = handle)}
          style={{ flex: 1 }}
        >
          {(item, index) => (
            <View style={{ padding: 12 }}>
              <Text style={{ fontSize: 14, color: "#0f172a" }}>
                {item().title} (index: {index()})
              </Text>
            </View>
          )}
        </List>
      </View>
      <View style={{ flexDirection: "row", gap: 8, marginTop: 8 }}>
        <View
          testID="scroll-to-end"
          style={{ padding: 8, backgroundColor: "#3b82f6", borderRadius: 6 }}
          onPress={() => listRef?.scrollToEnd({ animated: true })}
        >
          <Text style={{ color: "#ffffff", fontSize: 12 }}>Scroll to End</Text>
        </View>
        <View
          testID="scroll-to-top"
          style={{ padding: 8, backgroundColor: "#3b82f6", borderRadius: 6 }}
          onPress={() => listRef?.scrollToIndex(0, { animated: true })}
        >
          <Text style={{ color: "#ffffff", fontSize: 12 }}>Scroll to Top</Text>
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
      <Text style={{ fontSize: 14, color: "#64748b", marginBottom: 8 }}>
        Tap to animate, long-press for different action.
      </Text>
      <View
        testID="animated-box"
        style={{
          width: 100,
          height: 100,
          backgroundColor: "#3b82f6",
          borderRadius: 12,
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
      {longPressed() && (
        <Text style={{ fontSize: 14, color: "#10b981", marginTop: 8 }}>
          Long pressed!
        </Text>
      )}
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
    }
  );

  return (
    <Section title="Resource">
      <Text style={{ fontSize: 14, color: "#64748b", marginBottom: 8 }}>
        State: {data.state()} {data.loading() && "(loading)"}
      </Text>
      {data.error() && (
        <Text style={{ fontSize: 14, color: "#ef4444" }}>
          Error: {String(data.error())}
        </Text>
      )}
      {data() && (
        <Text style={{ fontSize: 14, color: "#0f172a" }}>
          {JSON.stringify(data(), null, 2)}
        </Text>
      )}
      <View
        testID="refetch"
        style={{ padding: 8, backgroundColor: "#3b82f6", borderRadius: 6, marginTop: 8 }}
        onPress={() => data.refetch()}
      >
        <Text style={{ color: "#ffffff", fontSize: 12 }}>Refetch</Text>
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
      <Text style={{ fontSize: 14, color: "#64748b", marginBottom: 8 }}>
        Stored: {storedValue()}
      </Text>
      <Input
        testID="storage-input"
        value={inputValue()}
        onChange={inputValue.set}
        placeholder="Enter value to store"
        style={{ padding: 8, borderWidth: 1, borderColor: "#cbd5e1", borderRadius: 6 }}
      />
      <View style={{ flexDirection: "row", gap: 8, marginTop: 8 }}>
        <View
          testID="save-storage"
          style={{ padding: 8, backgroundColor: "#3b82f6", borderRadius: 6 }}
          onPress={save}
        >
          <Text style={{ color: "#ffffff", fontSize: 12 }}>Save</Text>
        </View>
        <View
          testID="clear-storage"
          style={{ padding: 8, backgroundColor: "#ef4444", borderRadius: 6 }}
          onPress={clear}
        >
          <Text style={{ color: "#ffffff", fontSize: 12 }}>Clear</Text>
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
      <Text style={{ fontSize: 14, color: "#64748b" }}>
        Current scheme: {scheme()} (change system settings to see update)
      </Text>
      <View
        style={{
          padding: 16,
          backgroundColor: scheme() === "dark" ? "#1e293b" : "#f8fafc",
          borderRadius: 8,
          marginTop: 8,
        }}
      >
        <Text style={{ color: scheme() === "dark" ? "#f1f5f9" : "#0f172a" }}>
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
      <Text style={{ fontSize: 14, color: "#64748b" }}>
        Press back button (Android) or browser back to trigger.
      </Text>
      {handled() && (
        <Text style={{ fontSize: 14, color: "#10b981", marginTop: 8 }}>
          Back handled!
        </Text>
      )}
    </Section>
  );
}

function App() {
  return (
    <Scroll testID="page" style={{ flex: 1, backgroundColor: "#f8fafc" }}>
      <View
        testID="safe"
        style={{
          paddingTop: "calc(env(safe-area-inset-top) + 16px)",
          paddingRight: "calc(env(safe-area-inset-right) + 16px)",
          paddingBottom:
            "calc(max(env(safe-area-inset-bottom), env(keyboard-inset-height)) + 32px)",
          paddingLeft: "calc(env(safe-area-inset-left) + 16px)",
        }}
      >
        <Text style={{ fontSize: 28, fontWeight: "700", color: "#0f172a" }}>Gallery</Text>
        <Text style={{ fontSize: 14, color: "#64748b" }}>
          View, Text, Image, Scroll, Input, List, Animations, Resource, Storage, and more.
        </Text>
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
      </View>
    </Scroll>
  );
}

mount(App);
