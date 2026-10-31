import { mount } from "@nlayer/runtime";
import { Animated, Scroll, Text, View, createAnimatedValue } from "@nlayer/ui";

const panX = createAnimatedValue(0);
const panY = createAnimatedValue(0);
const pinchScale = createAnimatedValue(1);
const rotationAngle = createAnimatedValue(0);

function App() {
  return (
    <View style={{ flex: 1 }}>
      {/* Pan gesture: horizontal drag */}
      <View
        testID="panCard"
        pan={{ x: panX }}
        style={{
          width: 100,
          height: 100,
          backgroundColor: "#ff0000",
          transform: [{ translateX: panX }],
        }}
        onPanEnd={(e) => {
          Animated.spring(panX, { toValue: 0, velocity: e.velocity.x });
        }}
      />
      <Text testID="panValue">{`x: ${panX}`}</Text>

      {/* Scroll with horizontal pan card inside */}
      <Scroll testID="scroll" style={{ height: 200 }}>
        <View style={{ height: 400 }}>
          <View
            testID="scrollPanCard"
            pan={{ x: createAnimatedValue(0) }}
            style={{ width: 100, height: 50, backgroundColor: "#00ff00" }}
          />
          <Text>Scrollable content</Text>
        </View>
      </Scroll>

      {/* Pinch/rotation (bindings exist but headless doesn't drive them) */}
      <View
        testID="pinchCard"
        pinch={{ scale: pinchScale }}
        style={{ width: 50, height: 50, backgroundColor: "#0000ff" }}
      />
      <View
        testID="rotationCard"
        rotation={{ angle: rotationAngle }}
        style={{ width: 50, height: 50, backgroundColor: "#ffff00" }}
      />
    </View>
  );
}

mount(App);
