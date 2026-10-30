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
const border = createAnimatedValue(0);
const borderColor = border.interpolate({
  inputRange: [0, 1],
  outputRange: ["#000000", "#00ff00"],
});
const spin = createAnimatedValue(0);
const grow = createAnimatedValue(1);
const stopValue = createAnimatedValue(0);
const stopped = signal("running");
const done = signal("running");
const finished = value.timing({ toValue: 1, duration: 300 });
finished.then((result) => done.set(result.finished ? "done" : "unfinished"));
Animated.spring(background, { toValue: 1, stiffness: 180, damping: 20 });
border.timing({ toValue: 1, duration: 300 });
spin.timing({ toValue: 45, duration: 300 });
grow.timing({ toValue: 2, duration: 300 });
const stopping = stopValue.timing({ toValue: 1, duration: 1000 });
stopValue.stop();
stopping.then((result) => stopped.set(result.finished ? "finished" : "stopped"));

function App() {
  return (
    <View
      testID="box"
      style={{
        width: 100,
        height: 100,
        opacity,
        backgroundColor: color,
        borderColor,
        borderWidth: 1,
        transform: [{ translateX: value }, { rotate: spin }, { scale: grow }],
      }}
    >
      <Text>{`${done()} ${stopped()}`}</Text>
    </View>
  );
}

mount(App);
