import { mount, resource, signal } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

declare const __nlayerTestOrigin: string | undefined;

const id = signal(0);
const fail = signal(false);
const n = signal(1);
const aborts = signal(0);
const user = resource(id, (value, info) => {
  info.signal.addEventListener("abort", () => aborts.set(aborts() + 1));
  if (fail()) return Promise.reject(new Error("boom"));
  const origin = typeof __nlayerTestOrigin === "string" ? __nlayerTestOrigin : "";
  return fetch(`${origin}/quote.json`)
    .then((response) => response.json())
    .then((body) => `${body.message} ${n()}`);
});

function App() {
  return (
    <View style={{ flex: 1 }}>
      <Text>{`${user.state()} ${user() ?? "-"} aborts ${aborts()}`}</Text>
      <View testID="load" style={{ width: 80, height: 40 }} onPress={() => id.set(1)}>
        <Text>Load</Text>
      </View>
      <View
        testID="fail"
        style={{ width: 80, height: 40 }}
        onPress={() => {
          fail.set(true);
          user.refetch();
        }}
      >
        <Text>Fail</Text>
      </View>
      <View
        testID="again"
        style={{ width: 80, height: 40 }}
        onPress={() => {
          fail.set(false);
          n.set(n() + 1);
          user.refetch();
        }}
      >
        <Text>Again</Text>
      </View>
    </View>
  );
}
mount(App);
