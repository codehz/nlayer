import { mount, signal } from "@nlayer/runtime";
import { Text, View, platform } from "@nlayer/ui";

function App() {
  const presses = signal(0);
  return (
    <View
      onPress={() => presses.set(presses() + 1)}
      style={{ width: 390, height: 844, padding: 8 }}
    >
      <View
        accessibilityLabel="inner"
        accessibilityState={{ disabled: true, selected: true }}
        disabled
        role="button"
        style={{ width: 100, height: 50, backgroundColor: "red" }}
      />
      <Text>{`scheme:${platform.colorScheme()} presses:${presses()}`}</Text>
    </View>
  );
}

mount(App);
