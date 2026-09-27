import { StrictMode, useRef, useState, type ChangeEvent, type FormEvent } from 'react';
import { createRoot } from 'react-dom/client';
import { LensoApiError, createLensoWebClient, unwrap } from '@lenso/web-client';
import type { components, paths } from './generated/api';
import { processQueuedJob, type JobState } from './process-job';
import './styles.css';

type Note = components['schemas']['Note'];

type BusinessSettings = { excerpt_limit: number; revision: number };
type Attachment = { id: string; filename: string; media_type: string; note_id: string; size: number };

function App() {
  const token = useRef('');
  const sessionEpoch = useRef(0);
  const noteEpoch = useRef<number | null>(null);
  const noteForm = useRef<HTMLFormElement>(null);
  const recoveryForm = useRef<HTMLFormElement>(null);
  const [note, setNote] = useState<Note>();
  const [status, setStatus] = useState('Enter an issued API token to begin.');
  const [recoveryStatus, setRecoveryStatus] = useState<string>();
  const [submitting, setSubmitting] = useState(false);
  const [retrieving, setRetrieving] = useState(false);
  const [settingsLimit, setSettingsLimit] = useState(96);
  const [settingsRevision, setSettingsRevision] = useState(1);

  function currentSession() {
    const epoch = sessionEpoch.current;
    const credential = token.current;
    return {
      epoch,
      api: createLensoWebClient<paths>({
        baseUrl: window.location.origin,
        authentication: { kind: 'bearer', accessToken: () => credential || undefined },
      }),
    };
  }

  function isCurrentSession(epoch: number) {
    return epoch === sessionEpoch.current;
  }

  function requireCurrentSession(epoch: number) {
    if (!isCurrentSession(epoch)) throw new Error('Credential changed during request');
  }

  function problem(error: unknown) {
    return error instanceof LensoApiError
      ? `${error.problem.code ?? 'request_failed'}: ${error.message}`
      : error instanceof Error ? error.message : String(error);
  }

  async function loadSession(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const { api, epoch } = currentSession();
    try {
      const settings = unwrap<BusinessSettings>(await api.GET('/settings'));
      if (!isCurrentSession(epoch)) return;
      setSettingsLimit(settings.excerpt_limit);
      setSettingsRevision(settings.revision);
      setStatus(`Credential verified; loaded excerpt policy revision ${settings.revision}.`);
    } catch (error) {
      if (isCurrentSession(epoch)) setStatus(problem(error));
    }
  }

  async function updateSettings(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const { api, epoch } = currentSession();
    const data = new FormData(event.currentTarget);
    try {
      const settings = unwrap<BusinessSettings>(await api.PUT('/settings', {
        body: {
          excerpt_limit: Number(data.get('excerpt_limit')),
          predecessor_revision: settingsRevision,
        },
      }));
      if (!isCurrentSession(epoch)) return;
      setSettingsLimit(settings.excerpt_limit);
      setSettingsRevision(settings.revision);
      setStatus(`Excerpt policy updated to ${settings.excerpt_limit} characters at revision ${settings.revision}.`);
    } catch (error) {
      if (isCurrentSession(epoch)) setStatus(problem(error));
    }
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const { api, epoch } = currentSession();
    setSubmitting(true);
    noteEpoch.current = null;
    setNote(undefined);
    setRecoveryStatus(undefined);
    setStatus('Creating…');
    const data = new FormData(event.currentTarget);
    let savedNote: Note | undefined;
    try {
      const created = unwrap<Note>(await api.POST('/notes', {
        body: { title: String(data.get('title') ?? ''), body: String(data.get('body') ?? '') },
      }));
      if (!isCurrentSession(epoch)) return;
      savedNote = created;
      noteEpoch.current = epoch;
      setNote(created);
      if (created.processing_status === 'queued') {
        setStatus('Queued; processing durable excerpt job…');
        await processQueuedJob(created.job_id, {
          inspect: async (jobId) => {
            requireCurrentSession(epoch);
            const result = unwrap<JobState>(await api.GET('/job-status/{job_id}', {
              params: { path: { job_id: jobId } },
            }));
            requireCurrentSession(epoch);
            return result;
          },
          claim: async () => {
            requireCurrentSession(epoch);
            const result = unwrap<{ processed: boolean }>(await api.POST('/jobs/process-next'));
            requireCurrentSession(epoch);
            return result;
          },
          isRetryableClaimError: (error) => error instanceof LensoApiError && error.response.status === 502,
        });
      }
      if (!isCurrentSession(epoch)) return;
      const read = unwrap<Note>(await api.GET('/notes/{note_id}', {
        params: { path: { note_id: created.id } },
      }));
      if (!isCurrentSession(epoch)) return;
      setNote(read);
      setStatus('Created, processed, and read back through the typed public API.');
    } catch (error) {
      if (isCurrentSession(epoch)) {
        setStatus(savedNote ? `Note ${savedNote.id} was saved; follow-up did not complete: ${problem(error)}` : problem(error));
      }
    } finally {
      if (isCurrentSession(epoch)) setSubmitting(false);
    }
  }

  async function retrieve(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const noteId = String(new FormData(event.currentTarget).get('note_id') ?? '').trim();
    if (!noteId) {
      setRecoveryStatus('Enter a note ID.');
      return;
    }
    const { api, epoch } = currentSession();
    setRetrieving(true);
    noteEpoch.current = null;
    setNote(undefined);
    setRecoveryStatus('Opening note…');
    try {
      const read = unwrap<Note>(await api.GET('/notes/{note_id}', {
        params: { path: { note_id: noteId } },
      }));
      if (!isCurrentSession(epoch)) return;
      noteEpoch.current = epoch;
      setNote(read);
      setRecoveryStatus(`Opened note ${read.id}.`);
    } catch (error) {
      if (isCurrentSession(epoch)) setRecoveryStatus(`Could not open note ${noteId}: ${problem(error)}`);
    } finally {
      if (isCurrentSession(epoch)) setRetrieving(false);
    }
  }

  async function upload(event: ChangeEvent<HTMLInputElement>) {
    const input = event.currentTarget;
    const file = input.files?.[0];
    const { api, epoch } = currentSession();
    if (!file || !note || noteEpoch.current !== epoch) return;
    const noteId = note.id;
    setStatus('Uploading attachment…');
    try {
      const content_base64 = await fileBase64(file);
      if (!isCurrentSession(epoch)) return;
      const attachment = unwrap<Attachment>(await api.POST('/note-attachments/{note_id}', {
        params: { path: { note_id: noteId } },
        body: { content_base64, filename: file.name, media_type: file.type || 'application/octet-stream' },
      }));
      if (!isCurrentSession(epoch)) return;
      setStatus(`Stored ${attachment.filename} (${attachment.size} bytes) for this user.`);
    } catch (error) {
      if (isCurrentSession(epoch)) setStatus(problem(error));
    } finally {
      input.value = '';
    }
  }

  return <main>
    <p className="eyebrow">Lenso reference app</p>
    <h1>Knowledge base</h1>
    <p className="lede">A normal React interface calling the App’s authenticated, explicitly public, generated TypeScript API.</p>
    <form className="panel" aria-labelledby="session-heading" onSubmit={loadSession}>
      <h2 id="session-heading">Session</h2>
      <label>API token <input type="password" autoComplete="off" onChange={(event) => {
        token.current = event.currentTarget.value.trim();
        sessionEpoch.current += 1;
        noteEpoch.current = null;
        setNote(undefined);
        setSettingsLimit(96);
        setSettingsRevision(1);
        setSubmitting(false);
        setRetrieving(false);
        noteForm.current?.reset();
        recoveryForm.current?.reset();
        setRecoveryStatus(undefined);
        setStatus(token.current ? 'Credential ready; requests are user-isolated.' : 'Enter an issued API token to begin.');
      }} /></label>
      <button type="submit">Load workspace</button>
    </form>
    <form onSubmit={updateSettings}>
      <h2>Excerpt policy</h2>
      <label>Character limit <input name="excerpt_limit" type="number" min="16" max="512" value={settingsLimit} onChange={(event) => setSettingsLimit(Number(event.currentTarget.value))} required /></label>
      <button type="submit">Update revision {settingsRevision}</button>
    </form>
    <form ref={noteForm} onSubmit={submit}>
      <label>Title <input name="title" autoComplete="off" required /></label>
      <label>Body <textarea name="body" required /></label>
      <button disabled={submitting || retrieving} type="submit">{submitting ? 'Creating…' : 'Create note'}</button>
    </form>
    <form ref={recoveryForm} aria-labelledby="recovery-heading" onSubmit={retrieve}>
      <h2 id="recovery-heading">Open an existing note</h2>
      <label>Note ID <input name="note_id" autoComplete="off" spellCheck={false} aria-describedby="recovery-status" required /></label>
      <button disabled={retrieving || submitting} type="submit">{retrieving ? 'Opening…' : 'Open note'}</button>
      <p id="recovery-status" className="status" role="status" aria-live="polite">
        {recoveryStatus ?? (note ? 'A note is open.' : 'No note open. Enter its ID to reopen it.')}
      </p>
    </form>
    <p className="status" role="status" aria-live="polite">{status}</p>
    {note && <article>
      <p className="eyebrow">{note.id}</p>
      <h2>{note.title}</h2>
      <p>{note.body}</p>
      <p><strong>Processing:</strong> {note.processing_status} · {note.job_id}</p>
      <p><strong>TypeScript excerpt:</strong> {note.excerpt}</p>
      <label>Attach a file <input type="file" onChange={upload} /></label>
    </article>}
  </main>;
}

function fileBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error('Could not read file'));
    reader.onload = () => resolve(String(reader.result).split(',', 2)[1] ?? '');
    reader.readAsDataURL(file);
  });
}

createRoot(document.getElementById('root')!).render(<StrictMode><App /></StrictMode>);
