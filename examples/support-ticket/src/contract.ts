export const SUPPORT_TICKET_CONTRACT_DIGEST =
  "sha256:5c95d669efa62fa3b423bc46a5e9be3af17393b6c97cb57a9966e3bb79be1155";

export const SUPPORT_TICKET_OPERATION_IDS = {
  close: "support-ticket/http/POST:/tickets/{id}/close",
  create: "support-ticket/http/POST:/tickets",
  detail: "support-ticket/http/GET:/tickets/{id}",
  list: "support-ticket/http/GET:/tickets",
  restrictedDetail: "support-ticket/http/GET:/tickets/{id}/restricted",
  update: "support-ticket/http/PATCH:/tickets/{id}",
} as const;

export const SUPPORT_TICKET_SURFACE_GRANT_OPERATION_IDS = [
  SUPPORT_TICKET_OPERATION_IDS.list,
  SUPPORT_TICKET_OPERATION_IDS.restrictedDetail,
  SUPPORT_TICKET_OPERATION_IDS.update,
  SUPPORT_TICKET_OPERATION_IDS.create,
  SUPPORT_TICKET_OPERATION_IDS.close,
] as const;
