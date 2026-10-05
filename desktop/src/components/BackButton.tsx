import { FiArrowLeft } from "solid-icons/fi";
import { onEscape } from "../lib/onEscape";
import "./BackButton.css";

/**
 * The one Back control for full-window flows (onboarding): pinned to the
 * top-left, under the macOS traffic lights, so it sits where Settings
 * sub-screens put theirs (in .settings-subheader). Screens with internal steps
 * pass a handler that steps back within the screen first. Esc triggers it too.
 */
export default function BackButton(props: { onClick: () => void; label?: string }) {
  // Esc does what Back does while this screen is showing.
  onEscape(() => props.onClick());
  return (
    <button class="back-btn screen-back" onClick={() => props.onClick()}>
      <FiArrowLeft size={14} />
      {props.label ?? "Back"}
    </button>
  );
}
