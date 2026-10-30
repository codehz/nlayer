import { mount, signal } from "@nlayer/runtime";
import { Animated, Text, View, createAnimatedValue } from "@nlayer/ui";

const value = createAnimatedValue(0);
const opacity = value.interpolate({
  inputRange: [0, 1],
  outputRange: [0, 1],
});
const background = createAnimatedValue(0);
const color = background.interpolate({
  inputRange: [0, 1],
  outputRange: ["#000000", "#ff0000"],
});
const done = signal("running");
const finished = value.timing({ toValue: 1, duration: 300 });
finished.then((result) => done.set(result.finished ? "done" : "stopped"));
Animated.spring(background, { toValue: 1, stiffness: 180, damping: 20 });

function App() {
  return (
    <View testID="box" style={{ width: 100, height: 100, opacity, backgroundColor: color }}>
      <Text>{done()}</Text>
    </View>
  );
}

mount(App);
