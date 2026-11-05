import { Text, View } from "@nlayer/ui";

export function Section(props: { title: string; children?: unknown }) {
  return (
    <View class="flex flex-col gap-2 pt-5">
      <Text class="text-xs font-semibold text-[#64748b]">{props.title}</Text>
      <View class="flex flex-col gap-2.5 rounded-xl border border-slate-200 bg-white p-3">
        {props.children}
      </View>
    </View>
  );
}
