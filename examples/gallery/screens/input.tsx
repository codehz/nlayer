import { signal } from "@nlayer/runtime";
import { Input, NavPage, Scroll, Text, View } from "@nlayer/ui";
import { dismiss } from "../nav";
import { Section } from "../Section";

function Field(props: {
  label: string;
  value: string;
  placeholder: string;
  onChange?: (e: { value: string }) => void;
  secureTextEntry?: boolean;
  keyboardType?: "text" | "email" | "number" | "phone";
  disabled?: boolean;
}) {
  return (
    <View class="flex flex-col gap-1">
      <Text class="text-xs text-[#64748b]">{props.label}</Text>
      <Input
        value={props.value}
        placeholder={props.placeholder}
        onChange={props.onChange}
        secureTextEntry={props.secureTextEntry}
        keyboardType={props.keyboardType}
        disabled={props.disabled}
        class="rounded-lg border border-[#cbd5e1] bg-white p-2 text-base text-[#0f172a]"
        classList={{ "bg-slate-100": props.disabled }}
      />
    </View>
  );
}

function Fields() {
  const name = signal("");
  const email = signal("");
  const amount = signal("");
  const phone = signal("");
  const secret = signal("");
  const locked = signal("locked");
  return (
    <Section title="Input">
      <Field
        label={`Name: ${name()}`}
        value={name()}
        placeholder="Name"
        onChange={({ value }) => name.set(value)}
      />
      <Field
        label="Email"
        value={email()}
        placeholder="Email"
        keyboardType="email"
        onChange={({ value }) => email.set(value)}
      />
      <Field
        label="Amount"
        value={amount()}
        placeholder="Amount"
        keyboardType="number"
        onChange={({ value }) => amount.set(value)}
      />
      <Field
        label="Phone"
        value={phone()}
        placeholder="Phone"
        keyboardType="phone"
        onChange={({ value }) => phone.set(value)}
      />
      <Field
        label="Password"
        value={secret()}
        placeholder="Password"
        secureTextEntry
        onChange={({ value }) => secret.set(value)}
      />
      <Field label={`Locked: ${locked()}`} value={locked()} placeholder="Locked" disabled />
    </Section>
  );
}

export function InputPage() {
  return (
    <NavPage title="Input" onDismiss={dismiss}>
      <Scroll testID="input-page" class="flex-1 bg-slate-50">
        <View class="p-4">
          <Fields />
        </View>
      </Scroll>
    </NavPage>
  );
}
