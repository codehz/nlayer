import { mount } from "@nlayer/runtime";
import { List, Text, View } from "@nlayer/ui";

const rows = [0, 1, 2, 3];

function App() {
  return (
    <View style={{ width: 400, height: 400 }}>
      <List
        testID="list"
        each={rows}
        numColumns={2}
        estimatedItemSize={40}
        style={{ width: 400, height: 300 }}
      >
        {(item, index) => (
          <View testID={`cell-${index()}`} style={{ width: 200, height: 40 }}>
            <Text>{String(item())}</Text>
          </View>
        )}
      </List>
    </View>
  );
}

mount(App);
