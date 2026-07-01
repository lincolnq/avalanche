import { For } from "solid-js";
import "./DisappearingMessagesPicker.css";

interface Option {
  label: string;
  seconds: number;
}

// Mirrors iOS DisappearingMessagesPicker options exactly.
const OPTIONS: Option[] = [
  { label: "Off", seconds: 0 },
  { label: "30 seconds", seconds: 30 },
  { label: "5 minutes", seconds: 300 },
  { label: "1 hour", seconds: 3600 },
  { label: "8 hours", seconds: 28800 },
  { label: "1 day", seconds: 86400 },
  { label: "1 week", seconds: 604800 },
  { label: "4 weeks", seconds: 2419200 },
];

/** Human-readable label for a stored timer value. */
export function disappearingLabel(seconds: number): string {
  return OPTIONS.find((o) => o.seconds === seconds)?.label ?? `${seconds}s`;
}

interface Props {
  seconds: number;
  disabled?: boolean;
  onChange: (seconds: number) => void;
}

export default function DisappearingMessagesPicker(props: Props) {
  return (
    <select
      class="timer-picker"
      disabled={props.disabled}
      value={String(props.seconds)}
      onChange={(e) => props.onChange(Number(e.currentTarget.value))}
    >
      <For each={OPTIONS}>
        {(o) => <option value={String(o.seconds)}>{o.label}</option>}
      </For>
    </select>
  );
}
