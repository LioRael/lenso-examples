import { StrictMode, useMemo, useRef, useState, type ChangeEvent, type FormEvent } from 'react';
import { createRoot } from 'react-dom/client';
import { LensoApiError, createLensoWebClient, unwrap } from '@lenso/web-client';
import type { paths } from './generated/api';
import './styles.css';

type Note = {
  id: string;
  title: string;
  body: string;
  excerpt: string;
  job_id: string;
  processing_status: 'queued' | 'succeeded';
};

type BusinessSettings = { excerpt_limit: number; revision: number };
type Attachment = { id: string; filename: string; media_type: string; note_id: string; size: number };

function App() {
  const token = useRef('');
  const api = useMemo(() => createLensoWebClient<paths>({
    baseUrl: window.location.origin,
    authentication: { kind: 'bearer', accessToken: () => token.current || undefined },
  }), []);
  const [note, setNote] = useState<Note>();
  const [status, setStatus] = useState('Enter an issued API token to begin.');
  const [submitting, setSubmitting] = useState(false);
  const [settingsLimit, setSettingsLimit] = useState(96);
  const [settingsRevision, setSettingsRevision] = useState(1);

  function problem(error: unknown) {
    return error instanceof LensoApiError
      ? `${error.problem.code ?? 'request_failed'}: ${error.message}`
      : error instanceof Error ? error.message : String(error);
  }

  async function loadSession(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      const settings = unwrap<BusinessSettings>(await api.GET('/settings'));
      setSettingsLimit(settings.excerpt_limit);
      setSettingsRevision(settings.revision);
      setStatus(`Credential verified; loaded excerpt policy revision ${settings.revision}.`);
    } catch (error) {
      setStatus(problem(error));
    }
  }

  async function updateSettings(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    try {
      const settings = unwrap<BusinessSettings>(await api.PUT('/settings', {
        body: {
          excerpt_limit: Number(data.get('excerpt_limit')),
          predecessor_revision: settingsRevision,
        },
      }));
      setSettingsLimit(settings.excerpt_limit);
      setSettingsRevision(settings.revision);
      setStatus(`Excerpt policy updated to ${settings.excerpt_limit} characters at revision ${settings.revision}.`);
    } catch (error) {
      setStatus(problem(error));
    }
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitting(true);
    setNote(undefined);
    setStatus('Creating…');
    const data = new FormData(event.currentTarget);
    try {
      const created = unwrap<Note>(await api.POST('/notes', {
        body: { title: String(data.get('title') ?? ''), body: String(data.get('body') ?? '') },
      }));
      if (created.processing_status === 'queued') {
        setStatus('Queued; processing durable excerpt job…');
        unwrap(await api.POST('/jobs/process-next'));
      }
      const read = unwrap<Note>(await api.GET('/notes/{note_id}', {
        params: { path: { note_id: created.id } },
      }));
      setNote(read);
      setStatus('Created, processed, and read back through the typed public API.');
    } catch (error) {
      setStatus(problem(error));
    } finally {
      setSubmitting(false);
    }
  }

  async function upload(event: ChangeEvent<HTMLInputElement>) {
    const input = event.currentTarget;
    const file = input.files?.[0];
    if (!file || !note) return;
    setStatus('Uploading attachment…');
    try {
      const content_base64 = await fileBase64(file);
      const attachment = unwrap<Attachment>(await api.POST('/note-attachments/{note_id}', {
        params: { path: { note_id: note.id } },
        body: { content_base64, filename: file.name, media_type: file.type || 'application/octet-stream' },
      }));
      setStatus(`Stored ${attachment.filename} (${attachment.size} bytes) for this user.`);
    } catch (error) {
      setStatus(problem(error));
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
        setStatus(token.current ? 'Credential ready; requests are user-isolated.' : 'Enter an issued API token to begin.');
      }} /></label>
      <button type="submit">Load workspace</button>
    </form>
    <form onSubmit={updateSettings}>
      <h2>Excerpt policy</h2>
      <label>Character limit <input name="excerpt_limit" type="number" min="16" max="512" value={settingsLimit} onChange={(event) => setSettingsLimit(Number(event.currentTarget.value))} required /></label>
      <button type="submit">Update revision {settingsRevision}</button>
    </form>
    <form onSubmit={submit}>
      <label>Title <input name="title" autoComplete="off" required /></label>
      <label>Body <textarea name="body" required /></label>
      <button disabled={submitting} type="submit">{submitting ? 'Creating…' : 'Create note'}</button>
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
