import { mount, signal } from "@nlayer/runtime";
import { Image, Input, Scroll, Text, View } from "@nlayer/ui";

function App() {
  const count = signal(0);
  const name = signal("");
  const note = signal("idle");
  return (
    <View style={{ width: 200, height: 220 }}>
      <View testID="tap" onPress={() => count.set(count() + 1)} style={{ width: 160, height: 36 }}>
        <Text>Count: {count()}</Text>
      </View>
      <Input
        testID="name"
        value={name()}
        placeholder="Name"
        onChange={(value) => name.set(value)}
        style={{ height: 24 }}
      />
      <Scroll testID="list" style={{ width: 200, height: 40 }}>
        <View style={{ width: 200, height: 200 }} />
      </Scroll>
      <Text>
        see{" "}
        <Text testID="docs" onPress={() => note.set("opened")}>
          docs
        </Text>
      </Text>
      <Text>{note()}</Text>
      <View testID="pad" style={{ paddingTop: "env(safe-area-inset-top)", width: 80, height: 40 }}>
        <Text>pad</Text>
      </View>
      <Image
        testID="mark"
        src="https://example.invalid/a.png"
        style={{ width: 32, height: 32, borderRadius: 16 }}
      />
    </View>
  );
}
mount(App);
