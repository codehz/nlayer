import { mount, signal } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

const long = signal(0);
const short = signal(0);

function App() {
  return (
    <View
      testID="target"
      style={{ width: 200, height: 80 }}
      onPress={() => short.set(short() + 1)}
      onLongPress={() => long.set(long() + 1)}
      delayLongPress={100}
    >
      <Text>{`long:${long()} short:${short()}`}</Text>
    </View>
  );
}

mount(App);
