import { cleanup, effect, resource, signal } from "@nlayer/runtime";
import { NavPage, Scroll, Text, View } from "@nlayer/ui";
import { dismiss } from "../nav";
import { quoteUrl } from "../quote";
import { Section } from "../Section";

function Quote() {
  const status = signal("Loading quote");
  effect(() => {
    const controller = new AbortController();
    fetch(quoteUrl(), { signal: controller.signal })
      .then((response) => {
        if (!response.ok) throw new Error(String(response.status));
        return response.json();
      })
      .then((body) => {
        const quote = body as { title: string; count: number };
        status.set(`${quote.title}: ${quote.count}`);
      })
      .catch(() => status.set("Quote failed"));
    cleanup(() => controller.abort());
  });
  return (
    <Section title="Fetch">
      <Text>{status()}</Text>
    </Section>
  );
}

function DataFetch() {
  const trigger = signal(0);
  const data = resource(
    () => trigger(),
    async (_, { signal }) => {
      const response = await fetch(quoteUrl(), { signal });
      if (!response.ok) throw new Error("Failed to fetch");
      return response.json();
    },
  );

  return (
    <Section title="Resource">
      <Text class="mb-2 text-sm text-[#64748b]">
        State: {data.state()} {data.loading() && "(loading)"}
      </Text>
      {data.error() && <Text class="text-sm text-[#ef4444]">Error: {String(data.error())}</Text>}
      {data() && <Text class="text-sm text-[#0f172a]">{JSON.stringify(data(), null, 2)}</Text>}
      <View
        testID="refetch"
        class="mt-2 self-start rounded-md bg-[#3b82f6] p-2"
        onPress={() => data.refetch()}
      >
        <Text class="text-xs text-white">Refetch</Text>
      </View>
    </Section>
  );
}

export function FetchPage() {
  return (
    <NavPage title="Fetch" onDismiss={dismiss}>
      <Scroll testID="fetch-page" class="flex-1 bg-slate-50">
        <View class="pt-[calc(env(safe-area-inset-top)+16px)] pr-4 pb-4 pl-4">
          <Quote />
          <DataFetch />
        </View>
      </Scroll>
    </NavPage>
  );
}
