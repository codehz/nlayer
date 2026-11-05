import { Image, NavPage, Scroll, View } from "@nlayer/ui";
import mark from "../mark.png";
import { dismiss } from "../nav";
import { Section } from "../Section";

function Pictures() {
  return (
    <Section title="Image">
      <Image src={mark} class="size-8 rounded-md" />
      <Image src="https://example.com/photo.jpg" class="h-8 w-12 rounded-md object-contain" />
    </Section>
  );
}

export function ImagePage() {
  return (
    <NavPage title="Image" onDismiss={dismiss}>
      <Scroll testID="image-page" class="flex-1 bg-slate-50">
        <View class="p-4">
          <Pictures />
        </View>
      </Scroll>
    </NavPage>
  );
}
