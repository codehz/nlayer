import { mount, signal, storage } from "@nlayer/runtime";
import { Text, View } from "@nlayer/ui";

function App() {
  const draft = signal("loading");
  const quota = signal("");
  storage.getItem("draft").then((value) => draft.set(value === null ? "none" : value));
  return (
    <View style={{ flex: 1 }}>
      <Text>{draft()}</Text>
      <Text>{quota()}</Text>
      <View
        testID="reset"
        style={{ width: 80, height: 40 }}
        onPress={() => {
          storage.removeItem("draft").then(() => draft.set("none"));
        }}
      >
        <Text>Reset</Text>
      </View>
      <View
        testID="save"
        style={{ width: 80, height: 40 }}
        onPress={() => {
          storage.setItem("draft", "kept").then(() => draft.set("kept"));
        }}
      >
        <Text>Save</Text>
      </View>
      <View
        testID="full"
        style={{ width: 80, height: 40 }}
        onPress={() => {
          storage
            .setItem("quota", "x".repeat(5 * 1024 * 1024))
            .then(() => quota.set("wrote"))
            .catch((error) => quota.set(`${error.name} ${error.code}`));
        }}
      >
        <Text>Full</Text>
      </View>
    </View>
  );
}
mount(App);
