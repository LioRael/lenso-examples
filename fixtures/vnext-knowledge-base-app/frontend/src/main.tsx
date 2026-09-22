import { StrictMode, useState, type FormEvent } from 'react';
import { createRoot } from 'react-dom/client';
import { LensoApiError, createLensoWebClient, unwrap } from '@lenso/web-client';
import type { paths } from './generated/api';
import './styles.css';

const api = createLensoWebClient<paths>({ baseUrl: window.location.origin });

type Note = {
  id: string;
  title: string;
  body: string;
  excerpt: string;
  job_id: string;
  processing_status: 'queued' | 'succeeded';
};

function App() {
  const [note, setNote] = useState<Note>();
  const [status, setStatus] = useState('Ready to create a note.');
  const [submitting, setSubmitting] = useState(false);

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
        unwrap<Note>(await api.POST('/jobs/process-next'));
      }
      const read = unwrap<Note>(await api.GET('/notes/{note_id}', {
        params: { path: { note_id: created.id } },
      }));
      setNote(read);
      setStatus('Created, processed, and read back through the typed public API.');
    } catch (error) {
      setStatus(error instanceof LensoApiError
        ? `${error.problem.code ?? 'request_failed'}: ${error.message}`
        : error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(false);
    }
  }

  return <main>
    <p className="eyebrow">Lenso reference app</p>
    <h1>Knowledge base</h1>
    <p className="lede">A normal React interface calling the App’s explicitly public, generated TypeScript API.</p>
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
    </article>}
  </main>;
}

createRoot(document.getElementById('root')!).render(<StrictMode><App /></StrictMode>);
