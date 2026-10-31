import { mount, signal } from "@nlayer/runtime";
import { List, Text, View, type ListHandle } from "@nlayer/ui";

const rows = Array.from({ length: 5000 }, (_, i) => i);

function Driver(props: { name: string; label: string; onPress: () => void }) {
  return (
    <View testID={props.name} onPress={props.onPress} style={{ width: 390, height: 28 }}>
      <Text>{props.label}</Text>
    </View>
  );
}

function App() {
  const more = signal(0);
  const data = signal(rows);
  let list: ListHandle | undefined;
  return (
    <View style={{ width: 390, height: 760 }}>
      <List
        testID="list"
        ref={(handle) => {
          list = handle;
        }}
        each={data()}
        estimatedItemSize={51}
        loadMoreThreshold={1}
        onLoadMore={() => more.set(more() + 1)}
        style={{ width: 390, height: 400 }}
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
      <Driver name="to-top" label="top" onPress={() => list?.scrollToIndex(0)} />
      <Driver name="to-4000" label="4000" onPress={() => list?.scrollToIndex(4000)} />
      <Driver name="to-beyond" label="beyond" onPress={() => list?.scrollToIndex(999999)} />
      <Driver name="to-end" label="end" onPress={() => list?.scrollToEnd()} />
      <Driver
        name="reorder"
        label="reorder"
        onPress={() => data.set([3, 0, 1, 2, ...rows.slice(4)])}
      />
      <Driver name="prepend" label="prepend" onPress={() => data.set([-2, -1, ...data()])} />
      <Driver name="append" label="append" onPress={() => data.set([...data(), 5000, 5001])} />
      <Driver name="shrink" label="shrink" onPress={() => data.set(data().slice(0, 3))} />
      <Driver name="clear" label="clear" onPress={() => data.set([])} />
      <Driver name="restore" label="restore" onPress={() => data.set(rows)} />
      <Text testID="more">{`more:${more()}`}</Text>
      <Text testID="count">{`count:${data().length}`}</Text>
    </View>
  );
}

mount(App);
