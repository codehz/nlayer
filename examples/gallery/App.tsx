import { cleanup, effect, mount, signal } from "@nlayer/runtime";
import { Image, Input, Scroll, Text, View } from "@nlayer/ui";
import mark from "./mark.png";

declare global {
  var __nlayerTestOrigin: string | undefined;
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

function App() {
  return (
    <Scroll testID="page" style={{ flex: 1, backgroundColor: "#f8fafc" }}>
      <View
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
          View, Text, Image, Scroll, and a single-line Input.
        </Text>
        <Layout />
        <Pointer />
        <Type />
        <Pictures />
        <Scrollers />
        <ScrollEvents />
        <Fields />
        <Quote />
      </View>
    </Scroll>
  );
}

mount(App);
