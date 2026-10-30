import { mount } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

const rows = Array.from({ length: 100 }, (_, i) => i);

function App() {
  return (
    <View style={{ width: 300, height: 300 }}>
      <List
        testID="list"
        each={rows}
        initialScrollIndex={50}
        estimatedItemSize={30}
        style={{ width: 300, height: 300 }}
      >
        {(item, index) => (
          <View testID={`row-${index()}`} style={{ width: 300, height: 30 }}>
            <Text>{String(item())}</Text>
          </View>
        )}
      </List>
    </View>
  );
}

mount(App);
