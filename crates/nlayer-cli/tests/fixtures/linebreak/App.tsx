import { mount, signal } from "@nlayer/runtime";
import { Input, Text, View } from "@nlayer/ui";

function App() {
  const value = signal("");
  return (
    <View style={{ width: 390, height: 844, padding: 8 }}>
      <Input
        testID="field"
        multiline
        value={value()}
        onChange={(text) => value.set(text)}
        style={{ width: 300, height: 120 }}
      />
      <Text>{value()}</Text>
    </View>
  );
}

mount(App);
