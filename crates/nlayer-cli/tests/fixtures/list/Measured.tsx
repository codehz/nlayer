import { mount } from "@nlayer/runtime";
import { List, Text, View, type ListHandle } from "@nlayer/ui";

// 40, 60, 80, repeating: no two rows are the same height.
const items = Array.from({ length: 40 }, (_, i) => i);
const heightOf = (item: number) => 40 + (item % 3) * 20;

function Driver(props: { name: string; label: string; onPress: () => void }) {
  return (
    <View testID={props.name} onPress={props.onPress} style={{ width: 390, height: 30 }}>
      <Text>{props.label}</Text>
    </View>
  );
}

function App() {
  let one: ListHandle | undefined;
  let two: ListHandle | undefined;
  return (
    <View style={{ width: 390, height: 660 }}>
      <List
        testID="single"
        ref={(handle) => {
          one = handle;
        }}
        each={items}
        estimatedItemSize={30}
        style={{ width: 390, height: 300 }}
        header={
          <View testID="single-head" style={{ width: 390, height: 64 }}>
            <Text>Head</Text>
          </View>
        }
        footer={
          <View testID="single-foot" style={{ width: 390, height: 44 }}>
            <Text>Foot</Text>
          </View>
        }
      >
        {(item, index) => (
          <View testID={`single-${index()}`} style={{ width: 390, height: heightOf(item()) }}>
            <Text>{`item ${item()}`}</Text>
          </View>
        )}
      </List>
      <Driver name="one-to-20" label="one 20" onPress={() => one?.scrollToIndex(20)} />
      <Driver name="one-to-39" label="one 39" onPress={() => one?.scrollToIndex(39)} />
      <List
        testID="columns"
        ref={(handle) => {
          two = handle;
        }}
        each={items}
        numColumns={2}
        estimatedItemSize={30}
        style={{ width: 390, height: 300 }}
      >
        {(item, index) => (
          <View testID={`pair-${index()}`} style={{ width: 195, height: heightOf(item()) }}>
            <Text>{`item ${item()}`}</Text>
          </View>
        )}
      </List>
      <Driver name="two-to-20" label="two 20" onPress={() => two?.scrollToIndex(20)} />
    </View>
  );
}

mount(App);
