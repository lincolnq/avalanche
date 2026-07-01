import type { AttachmentFfi, LinkPreviewFfi } from "../bindings";

export enum DeliveryStatus {
  sending = 0,
  sent = 1,
  delivered = 2,
  read = 3,
  failed = 4,
}

export interface Message {
  id: string;
  conversationId: string;
  senderAccountId: string;   // NOT senderId or authorId
  body: string;              // NOT content or text
  sentAtMs: number;          // unix-ms Int64 — NOT a Date
  editedAtMs?: number;
  readAtMs?: number;
  deliveryStatus: DeliveryStatus;
  editCount: number;
  isDeleted: boolean;
  kind: number;
  metadata?: string;
  expireTimerSecs: number;
  expireAtMs?: number;
  // Attachments (docs/35) and link-preview cards on this message. Absent on
  // plain-text messages; treat undefined as empty when rendering.
  attachments?: AttachmentFfi[];
  previews?: LinkPreviewFfi[];
}
