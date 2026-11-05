import { cleanup, effect, signal } from "@nlayer/runtime";
import { BackHandler, NavPage, Scroll, Text, View, platform } from "@nlayer/ui";
import { dismiss } from "../nav";
import { Section } from "../Section";

function Surface() {
  return (
    <Section title="Platform">
      <View class="flex flex-row items-center justify-between">
        <Text class="text-xs text-[#64748b]">platform.os</Text>
        <Text>{platform.os}</Text>
      </View>
    </Section>
  );
}

function ColorScheme() {
  const scheme = signal(platform.colorScheme());

  effect(() => {
    scheme.set(platform.colorScheme());
  });

  return (
    <Section title="Color Scheme">
      <Text class="text-sm text-[#64748b]">
        Current scheme: {scheme()} (change system settings to see update)
      </Text>
      <View
        class="mt-2 rounded-lg bg-slate-50 p-4"
        classList={{ "bg-[#1e293b]": scheme() === "dark" }}
      >
        <Text class="text-[#0f172a]" classList={{ "text-slate-100": scheme() === "dark" }}>
          This box adapts to {scheme()} mode
        </Text>
      </View>
    </Section>
  );
}

function BackNav() {
  const handled = signal(false);

  effect(() => {
    const remove = BackHandler.addEventListener(() => {
      handled.set(true);
      setTimeout(() => handled.set(false), 1000);
      return true; // handled
    });
    cleanup(remove);
  });

  return (
    <Section title="Back Handler">
      <Text class="text-sm text-[#64748b]">
        Press back button (Android) or browser back to trigger.
      </Text>
      {handled() && <Text class="mt-2 text-sm text-[#10b981]">Back handled!</Text>}
    </Section>
  );
}

export function PlatformPage() {
  return (
    <NavPage title="Platform" onDismiss={dismiss}>
      <Scroll testID="platform-page" class="flex-1 bg-slate-50">
        <View class="p-4">
          <Surface />
          <ColorScheme />
          <BackNav />
        </View>
      </Scroll>
    </NavPage>
  );
}
