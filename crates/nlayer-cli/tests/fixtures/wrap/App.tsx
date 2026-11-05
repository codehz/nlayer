import { mount, signal } from "@nlayer/runtime";
import { Scroll, Text, View, type TextStyle } from "@nlayer/ui";

/** Long enough that a 16px "M" run exceeds a phone-width border box. */
const COUNT = 48;
const LONG = "M".repeat(120);
const live = signal(LONG);

const text: TextStyle = { fontSize: 16 };
const pad: TextStyle = { ...text, paddingHorizontal: 16 };
const border: TextStyle = {
  ...text,
  borderLeftWidth: 12,
  borderRightWidth: 12,
  borderColor: "#111",
};
const left: TextStyle = { ...text, paddingLeft: 40 };
const both: TextStyle = {
  ...text,
  paddingHorizontal: 8,
  borderLeftWidth: 8,
  borderRightWidth: 8,
  borderColor: "#111",
};

function samples(prefix: string, style: TextStyle) {
  const nodes = [];
  for (let n = 1; n <= COUNT; n++) {
    nodes.push(
      <Text testID={`${prefix}${n}`} style={style}>
        {"M".repeat(n)}
      </Text>,
    );
  }
  return nodes;
}

function App() {
  return (
    <Scroll
      testID="page"
      style={{
        flex: 1,
        paddingLeft: "env(safe-area-inset-left)",
        paddingRight: "env(safe-area-inset-right)",
      }}
    >
      <Text testID="live" style={pad}>
        {live()}
      </Text>
      <Text testID="shrink" style={text} onPress={() => live.set("M")}>
        shrink
      </Text>
      <Text testID="grow" style={text} onPress={() => live.set(LONG)}>
        grow
      </Text>

      <Text testID="line" style={text}>
        M
      </Text>
      <Text testID="pline" style={pad}>
        M
      </Text>
      <Text testID="bline" style={border}>
        M
      </Text>
      <Text testID="aline" style={left}>
        M
      </Text>
      <Text testID="cline" style={both}>
        M
      </Text>

      <Text testID="clamp" style={{ ...pad, numberOfLines: 1 }}>
        {LONG}
      </Text>
      <Text testID="fixed" style={{ ...pad, height: 22 }}>
        {LONG}
      </Text>
      <Text testID="long" style={pad}>
        {LONG}
      </Text>
      <Text testID="vlong" style={{ ...text, paddingVertical: 12 }}>
        {LONG}
      </Text>
      <Text testID="ulong" style={text}>
        {LONG}
      </Text>

      <View testID="parent" style={{ paddingHorizontal: 24 }}>
        <Text testID="parent-long" style={text}>
          {LONG}
        </Text>
        <Text testID="parent-short" style={text}>
          M
        </Text>
      </View>

      <Text testID="definite" style={{ ...pad, width: 120 }}>
        {LONG}
      </Text>
      <Text testID="definite-short" style={{ ...pad, width: 120 }}>
        M
      </Text>

      <View style={{ flexDirection: "row", width: "100%" }}>
        <View style={{ width: 120, height: 8 }} />
        <Text testID="row" style={{ ...pad, flex: 1, minWidth: 0 }}>
          {LONG}
        </Text>
      </View>
      <View style={{ flexDirection: "row", width: "100%" }}>
        <View style={{ width: 120, height: 8 }} />
        <Text testID="row-short" style={{ ...pad, flex: 1, minWidth: 0 }}>
          M
        </Text>
      </View>

      <Text testID="min-short" style={{ ...text, minHeight: 30 }}>
        M
      </Text>
      <Text testID="min-long" style={{ ...pad, minHeight: 30 }}>
        {LONG}
      </Text>

      <Text testID="bold-short" style={{ ...pad, fontWeight: "bold" }}>
        M
      </Text>
      <Text testID="bold-long" style={{ ...pad, fontWeight: "bold" }}>
        {LONG}
      </Text>

      <Text testID="header-u" style={{ ...text, fontWeight: "600" }}>
        → Header Demo (title, headerStyle, headerTransparent)
      </Text>
      <Text testID="header-p" style={{ ...text, fontWeight: "600", padding: 8 }}>
        → Header Demo (title, headerStyle, headerTransparent)
      </Text>
      <Text testID="header-short" style={{ ...text, fontWeight: "600", padding: 8 }}>
        → Dialog Demo
      </Text>

      {samples("u", text)}
      {samples("p", pad)}
      {samples("b", border)}
      {samples("a", left)}
      {samples("c", both)}
    </Scroll>
  );
}

mount(App);
