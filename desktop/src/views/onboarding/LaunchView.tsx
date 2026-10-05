import wordmarkUrl from "../../assets/wordmark.svg";
import "./LaunchView.css";

/**
 * Shown from launch until saved accounts have been opened (or found absent),
 * so a returning user goes straight to their chats without the welcome screen
 * flashing first. The wordmark fades in only if opening takes a noticeable
 * moment; a fast launch shows just the background.
 */
export default function LaunchView() {
  return (
    <div class="launch" data-tauri-drag-region>
      <img class="launch-wordmark" src={wordmarkUrl} alt="Avalanche" draggable={false} />
    </div>
  );
}
