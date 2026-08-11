import type {
  ConsoleClient,
  SurfaceOperationRequestContext,
} from "@lenso/console-module-api";

import contract from "../../support-ticket/contracts/support-ticket-business-api.v1.json";

export const SUPPORT_TICKET_MODULE_ID = "support/tickets" as const;
export const SUPPORT_TICKET_CONTRACT_ID = "support-ticket-http" as const;
export const SUPPORT_TICKET_CONTRACT_VERSION = "v1" as const;
export const SUPPORT_TICKET_CONTRACT_DIGEST =
  "sha256:da9725e81bebf8eb73c29fbd5fc800996ef014b98fc2bb689e1763deeeda90ad" as const;

export const SUPPORT_TICKET_OPERATION_IDS = {
  close: "support-ticket/http/POST:/tickets/{id}/close",
  create: "support-ticket/http/POST:/tickets",
  detail: "support-ticket/http/GET:/tickets/{id}",
  list: "support-ticket/http/GET:/tickets",
  restrictedDetail: "support-ticket/http/GET:/tickets/{id}/restricted",
  update: "support-ticket/http/PATCH:/tickets/{id}",
} as const;

export type SupportTicketStatus = "open" | "pending" | "escalated" | "closed";
export type SupportTicketPriority = "low" | "normal" | "high";

export interface SupportTicket {
  readonly id: string;
  readonly title: string;
  readonly status: SupportTicketStatus;
  readonly priority: SupportTicketPriority;
  readonly assignee: string;
  readonly created_at: string;
  readonly updated_at: string;
}

export interface SupportTicketPage {
  readonly records: readonly SupportTicket[];
  readonly next_cursor: string | null;
}

export interface CreateSupportTicketInput {
  readonly title: string;
  readonly priority?: SupportTicketPriority;
  readonly assignee?: string;
}

export interface UpdateSupportTicketInput {
  readonly title?: string;
  readonly status?: SupportTicketStatus;
  readonly priority?: SupportTicketPriority;
  readonly assignee?: string;
}

export interface SupportTicketRequestOptions {
  readonly tenantId?: string;
  readonly deadlineUnixMs?: number;
  readonly idempotencyKey?: string;
  readonly story?: SurfaceOperationRequestContext["story"];
}

export interface SupportTicketApi {
  list(
    input?: { readonly limit?: number; readonly cursor?: string },
    options?: SupportTicketRequestOptions
  ): Promise<SupportTicketPage>;
  create(
    input: CreateSupportTicketInput,
    options?: SupportTicketRequestOptions
  ): Promise<{ readonly ticket: SupportTicket }>;
  detail(
    ticketId: string,
    options?: SupportTicketRequestOptions
  ): Promise<{ readonly ticket: SupportTicket }>;
  restrictedDetail(
    ticketId: string,
    options?: SupportTicketRequestOptions
  ): Promise<{ readonly ticket: SupportTicket }>;
  update(
    ticketId: string,
    input: UpdateSupportTicketInput,
    options?: SupportTicketRequestOptions
  ): Promise<{ readonly ticket: SupportTicket }>;
  close(
    ticketId: string,
    options?: SupportTicketRequestOptions
  ): Promise<{ readonly ticket: SupportTicket }>;
}

const createRequestContext = (
  options: SupportTicketRequestOptions | undefined,
  requiresIdempotency: boolean
): SurfaceOperationRequestContext => {
  const idempotencyKey =
    options?.idempotencyKey ??
    (requiresIdempotency ? crypto.randomUUID() : undefined);
  return {
    ...(options?.tenantId ? { tenantId: options.tenantId } : {}),
    deadlineUnixMs: options?.deadlineUnixMs ?? Date.now() + 10_000,
    ...(idempotencyKey ? { idempotencyKey } : {}),
    ...(options?.story ? { story: options.story } : {}),
  };
};

export const createSupportTicketApi = (
  client: ConsoleClient
): SupportTicketApi => {
  const invoke = async <Input, Output>(
    operationId: string,
    input: Input,
    options: SupportTicketRequestOptions | undefined,
    requiresIdempotency: boolean
  ): Promise<Output> => {
    const requestContext = createRequestContext(options, requiresIdempotency);
    const response = await client.surfaceApi.invoke<Input, Output>({
      context: client.managedServiceContext,
      contractDigest: SUPPORT_TICKET_CONTRACT_DIGEST,
      input,
      moduleId: client.identity.moduleId,
      moduleReleaseDigest: client.identity.moduleReleaseDigest,
      operationId,
      protocol: "lenso.console-surface-gateway.v1",
      requestContext,
      uiArtifactDigest: client.identity.uiArtifactDigest,
    });
    return response.output;
  };

  return {
    close: (ticketId, options) =>
      invoke(SUPPORT_TICKET_OPERATION_IDS.close, { ticketId }, options, true),
    create: (input, options) =>
      invoke(SUPPORT_TICKET_OPERATION_IDS.create, input, options, true),
    detail: (ticketId, options) =>
      invoke(SUPPORT_TICKET_OPERATION_IDS.detail, { ticketId }, options, false),
    list: (input, options) =>
      invoke(SUPPORT_TICKET_OPERATION_IDS.list, input ?? {}, options, false),
    restrictedDetail: (ticketId, options) =>
      invoke(
        SUPPORT_TICKET_OPERATION_IDS.restrictedDetail,
        { ticketId },
        options,
        false
      ),
    update: (ticketId, input, options) =>
      invoke(
        SUPPORT_TICKET_OPERATION_IDS.update,
        { ticketId, ...input },
        options,
        true
      ),
  };
};

export const supportTicketContract = contract;
