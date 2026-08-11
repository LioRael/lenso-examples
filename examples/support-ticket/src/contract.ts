export const SUPPORT_TICKET_CONTRACT_DIGEST =
  "sha256:5b319cc7b4dbfe965cca4f770d5dc32c7d5cac984b2f374286d62ce1b5d6f1f9";

export const SUPPORT_TICKET_OPERATION_IDS = {
  close: "support-ticket/http/POST:/tickets/{id}/close",
  create: "support-ticket/http/POST:/tickets",
  list: "support-ticket/http/GET:/tickets",
  update: "support-ticket/http/PATCH:/tickets/{id}",
} as const;
