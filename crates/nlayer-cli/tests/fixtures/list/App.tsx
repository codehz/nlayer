import { mount, signal } from "@nlayer/runtime";
import { List, Text, View, type ListHandle } from "@nlayer/ui";

const rows = Array.from({ length: 5000 }, (_, i) => i);

function App() {
  const more = signal(0);
  const data = signal(rows);
  let list: ListHandle | undefined;
  return (
    <View style={{ width: 390, height: 700 }}>
      <List
        testID="list"
        ref={(handle) => {
          list = handle;
        }}
        each={data()}
        estimatedItemSize={50}
        loadMoreThreshold={1}
        onLoadMore={() => more.set(more() + 1)}
        style={{ width: 390, height: 500 }}
        header={<Text testID="head">Head</Text>}
        footer={<Text testID="foot">Foot</Text>}
        empty={<Text testID="empty">None</Text>}
        separator={<View testID="sep" style={{ height: 1 }} />}
      >
        {(item, index) => (
          <View testID={`row-${index()}`} style={{ width: 390, height: 50 }}>
            <Text>{`row ${item()}`}</Text>
          </View>
        )}
      </List>
      <View testID="to-4000" onPress={() => list?.scrollToIndex(4000)} style={{ height: 40 }}>
        <Text>4000</Text>
      </View>
      <View testID="to-end" onPress={() => list?.scrollToEnd()} style={{ height: 40 }}>
        <Text>end</Text>
      </View>
      <View testID="clear" onPress={() => data.set([])} style={{ height: 40 }}>
        <Text>clear</Text>
      </View>
      <Text testID="more">{`more:${more()}`}</Text>
    </View>
  );
}

mount(App);
