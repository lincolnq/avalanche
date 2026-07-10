import { createSignal, onMount } from "solid-js";
import { FiX } from "solid-icons/fi";
import { useApp } from "../state/AppContext";
import type { Conversation } from "../models";
import { initials } from "../lib/format";
import DisappearingMessagesPicker from "./DisappearingMessagesPicker";
import "./ConversationInfoView.css";

interface Props {
  conversation: Conversation;
  onClose: () => void;
}

/**
 * DM conversation-info modal, reached by clicking the conversation header's
 * name/avatar (desktop parity with iOS's tap-title-to-open-detail pattern for
 * groups). Hosts the disappearing-messages timer — a desktop-only control the
 * other platforms don't yet surface. Mirrors GroupDetailView's backdrop+dialog
 * shape; the group equivalent is GroupDetailView.
 */
export default function ConversationInfoView(props: Props) {
  const app = useApp();
  const accountId = props.conversation.accountId;
  const recipientDid = props.conversation.recipientDid;
  const [timerSecs, setTimerSecs] = createSignal(0);

  onMount(() => {
    if (!recipientDid) return;
    void app
      .getConversationTimer(accountId, recipientDid)
      .then((s) => setTimerSecs(s ?? 0));
  });

  function changeTimer(secs: number) {
    if (!recipientDid) return;
    setTimerSecs(secs); // optimistic
    void app
      .setConversationTimer(accountId, recipientDid, secs === 0 ? null : secs)
      .finally(() => {
        // Re-read the authoritative stored value so the picker reverts if the
        // write failed (mirrors GroupDetailView's setExpiry reload-after-write).
        void app
          .getConversationTimer(accountId, recipientDid)
          .then((s) => setTimerSecs(s ?? 0));
      });
  }

  return (
    <div class="convinfo-backdrop" onClick={props.onClose}>
      <div class="convinfo" onClick={(e) => e.stopPropagation()}>
        <div class="convinfo-header">
          <span class="convinfo-heading">Conversation info</span>
          <button
            class="convinfo-close"
            onClick={props.onClose}
            aria-label="Close"
          >
            <FiX size={18} />
          </button>
        </div>

        <div class="convinfo-body scrollbar-thin">
          {/* Contact identity */}
          <section class="convinfo-section">
            <div class="convinfo-identity">
              <div class="convinfo-avatar">
                {initials(props.conversation.title)}
              </div>
              <div class="convinfo-identity-text">
                <span class="convinfo-name">{props.conversation.title}</span>
                <span class="convinfo-did">{recipientDid}</span>
              </div>
            </div>
          </section>

          {/* Disappearing messages */}
          <section class="convinfo-section">
            <div class="convinfo-section-label">Disappearing messages</div>
            <DisappearingMessagesPicker
              seconds={timerSecs()}
              onChange={changeTimer}
            />
          </section>
        </div>
      </div>
    </div>
  );
}
