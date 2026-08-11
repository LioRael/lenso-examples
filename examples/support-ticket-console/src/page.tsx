import {
  Button,
  ConsolePage,
  DataTable,
  EmptyState,
  Field,
  Input,
  StateView,
  useConsoleClient,
} from "@lenso/console-ui";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { FormEvent, ReactNode } from "react";

import { createSupportTicketApi } from "./business-api";
import type { SupportTicket, SupportTicketPriority } from "./business-api";

const errorMessage = (error: unknown): string =>
  error instanceof Error ? error.message : String(error);

export const SupportTicketsPage = () => {
  const client = useConsoleClient();
  const api = useMemo(() => createSupportTicketApi(client), [client]);
  const [tickets, setTickets] = useState<readonly SupportTicket[]>([]);
  const [status, setStatus] = useState<"loading" | "ready" | "error">(
    "loading"
  );
  const [error, setError] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [priority, setPriority] = useState<SupportTicketPriority>("normal");
  const [saving, setSaving] = useState(false);
  const [drafts, setDrafts] = useState<Record<string, string>>({});

  const loadTickets = useCallback(async () => {
    setStatus("loading");
    setError(null);
    try {
      const page = await api.list({ limit: 100 });
      setTickets(page.records);
      setStatus("ready");
    } catch (loadError) {
      setStatus("error");
      setError(errorMessage(loadError));
    }
  }, [api]);

  useEffect(() => {
    void loadTickets();
  }, [loadTickets]);

  const createTicket = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!title.trim()) {
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await api.create({ priority, title: title.trim() });
      setTitle("");
      await loadTickets();
    } catch (createError) {
      setError(errorMessage(createError));
    } finally {
      setSaving(false);
    }
  };

  const updateTicket = async (ticket: SupportTicket) => {
    const nextTitle = drafts[ticket.id] ?? ticket.title;
    if (!nextTitle.trim() || nextTitle === ticket.title) {
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await api.update(ticket.id, { title: nextTitle.trim() });
      await loadTickets();
    } catch (updateError) {
      setError(errorMessage(updateError));
    } finally {
      setSaving(false);
    }
  };

  const closeTicket = async (ticket: SupportTicket) => {
    setSaving(true);
    setError(null);
    try {
      await api.close(ticket.id);
      await loadTickets();
    } catch (closeError) {
      setError(errorMessage(closeError));
    } finally {
      setSaving(false);
    }
  };

  let ticketContent: ReactNode;
  if (status === "loading") {
    ticketContent = (
      <StateView
        description="Reading the connected Module Business API."
        title="Loading support tickets"
      />
    );
  } else if (status === "error") {
    ticketContent = (
      <StateView
        action={<Button onClick={() => void loadTickets()}>Retry</Button>}
        description={error ?? "The Support Ticket Module could not be reached."}
        title="Support tickets unavailable"
      />
    );
  } else if (tickets.length === 0) {
    ticketContent = (
      <EmptyState>
        <EmptyState.Title>No support tickets yet</EmptyState.Title>
        <EmptyState.Description>
          Create the first ticket to see the Module-owned data here.
        </EmptyState.Description>
      </EmptyState>
    );
  } else {
    ticketContent = (
      <DataTable>
        <DataTable.Head>
          <DataTable.Row>
            <DataTable.Header>Ticket</DataTable.Header>
            <DataTable.Header>Status</DataTable.Header>
            <DataTable.Header>Priority</DataTable.Header>
            <DataTable.Header>Actions</DataTable.Header>
          </DataTable.Row>
        </DataTable.Head>
        <DataTable.Body>
          {tickets.map((ticket) => (
            <DataTable.Row key={ticket.id}>
              <DataTable.Cell>
                <Input
                  aria-label={`Title for ${ticket.id}`}
                  onChange={(event) => {
                    const { value } = event.currentTarget;
                    setDrafts((current) => ({
                      ...current,
                      [ticket.id]: value,
                    }));
                  }}
                  value={drafts[ticket.id] ?? ticket.title}
                />
                <small>{ticket.id}</small>
              </DataTable.Cell>
              <DataTable.Cell>{ticket.status}</DataTable.Cell>
              <DataTable.Cell>{ticket.priority}</DataTable.Cell>
              <DataTable.Cell>
                <Button
                  disabled={saving}
                  onClick={() => void updateTicket(ticket)}
                >
                  Save
                </Button>{" "}
                <Button
                  disabled={saving || ticket.status === "closed"}
                  onClick={() => void closeTicket(ticket)}
                  variant="danger"
                >
                  Close
                </Button>
              </DataTable.Cell>
            </DataTable.Row>
          ))}
        </DataTable.Body>
      </DataTable>
    );
  }

  return (
    <ConsolePage data-page="support-tickets-page">
      <ConsolePage.Header>
        <ConsolePage.Heading>
          <ConsolePage.Eyebrow>Support Ticket Module</ConsolePage.Eyebrow>
          <ConsolePage.Title>Support tickets</ConsolePage.Title>
          <ConsolePage.Description>
            Work with the connected Support Ticket Module through its bounded
            Business API.
          </ConsolePage.Description>
        </ConsolePage.Heading>
        <ConsolePage.Actions>
          <Button disabled={saving} onClick={() => void loadTickets()}>
            Refresh
          </Button>
        </ConsolePage.Actions>
      </ConsolePage.Header>

      <ConsolePage.Body data-page-slot="support-tickets-page__body">
        <form onSubmit={(event) => void createTicket(event)}>
          <Field>
            <Field.Label htmlFor="support-ticket-title">New ticket</Field.Label>
            <Input
              id="support-ticket-title"
              onChange={(event) => {
                const { value } = event.currentTarget;
                setTitle(value);
              }}
              placeholder="What needs attention?"
              value={title}
            />
            <Field.Hint>
              Creation uses the Module&apos;s typed write operation.
            </Field.Hint>
          </Field>
          <Field>
            <Field.Label htmlFor="support-ticket-priority">
              Priority
            </Field.Label>
            <select
              id="support-ticket-priority"
              onChange={(event) => {
                const { value } = event.currentTarget;
                setPriority(value as SupportTicketPriority);
              }}
              value={priority}
            >
              <option value="low">Low</option>
              <option value="normal">Normal</option>
              <option value="high">High</option>
            </select>
          </Field>
          <Button
            disabled={saving || !title.trim()}
            type="submit"
            variant="primary"
          >
            Create ticket
          </Button>
        </form>

        {error ? <p role="alert">{error}</p> : null}
        {ticketContent}
      </ConsolePage.Body>
    </ConsolePage>
  );
};
