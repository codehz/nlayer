import { mount, signal } from "@nlayer/runtime";
import { Input, Text, View } from "@nlayer/ui";

function App() {
  const value = signal("");
  const caret = signal({ start: 0, end: 0 });
  const log = signal<string[]>([]);
  const note = (entry: string) => log.set([...log(), entry]);
  return (
    <View style={{ width: 390, height: 844, padding: 8 }}>
      <Input
        testID="field"
        multiline
        value={value()}
        onChange={(text) => value.set(text)}
        selectedRange={caret()}
        onSelectionChange={(selection) => {
          caret.set(selection);
          note(`sel:${selection.start}-${selection.end}`);
        }}
        onCompositionChange={(composition) =>
          note(`compose:${composition.composing}:${composition.text}`)
        }
        style={{ width: 300, height: 120 }}
      />
      <Text>{log().join(" ")}</Text>
    </View>
  );
}

mount(App);
