export type JobState = { attempts: number; jobId: string; status: string };

export type ProcessingNote = { id: string; job_id: string | null; processing_status: string };

export type JobOperations<Note extends ProcessingNote> = {
  inspect: (jobId: string) => Promise<JobState>;
  readNote: (noteId: string) => Promise<Note>;
};

export type JobTiming = {
  now?: () => number;
  sleep?: (milliseconds: number) => Promise<void>;
  timeoutMs?: number;
};

export async function waitForProcessedNote<Note extends ProcessingNote>(
  noteId: string,
  initialJobId: string | null,
  operations: JobOperations<Note>,
  timing: JobTiming = {},
): Promise<Note> {
  const now = timing.now ?? (() => performance.now());
  const sleep = timing.sleep ?? ((milliseconds: number) => new Promise<void>((resolve) => {
    setTimeout(resolve, milliseconds);
  }));
  const deadline = now() + (timing.timeoutMs ?? 10_000);
  let jobId = initialJobId;
  let lastJobStatus = 'awaiting dispatch';
  if (jobId !== null && (typeof jobId !== 'string' || jobId.length === 0)) {
    throw new Error(`Note ${noteId} has an invalid initial job ID`);
  }

  while (true) {
    const note = await operations.readNote(noteId);
    if (note.id !== noteId || (jobId !== null && note.job_id !== jobId)) {
      throw new Error(`Read back another note or job for ${noteId}`);
    }
    if (note.job_id !== null && (typeof note.job_id !== 'string' || note.job_id.length === 0)) {
      throw new Error(`Note ${noteId} has an invalid job ID`);
    }
    jobId = note.job_id;
    if (note.processing_status === 'failed') {
      throw new Error(`Excerpt processing failed for note ${noteId} (job ${lastJobStatus})`);
    }
    if (!['dispatch_pending', 'queued', 'succeeded'].includes(note.processing_status)) {
      throw new Error(`Unexpected processing status for note ${noteId}: ${note.processing_status}`);
    }
    if (jobId !== null) {
      if (jobId.startsWith('inline:') && note.processing_status === 'succeeded') return note;
      const job = await operations.inspect(jobId);
      if (job.jobId !== jobId) throw new Error(`Jobs returned another job for ${jobId}`);
      if (!['queued', 'running', 'succeeded', 'failed'].includes(job.status)) {
        throw new Error(`Unexpected job status for ${jobId}: ${job.status}`);
      }
      lastJobStatus = job.status;
      if (note.processing_status === 'succeeded') {
        if (job.status !== 'succeeded') {
          throw new Error(`Note ${noteId} succeeded before its job completed`);
        }
        return note;
      }
    } else if (note.processing_status === 'succeeded') {
      throw new Error(`Note ${noteId} succeeded without a job ID`);
    }
    if (now() >= deadline) {
      throw new Error(`Note ${noteId} did not finish before the deadline (job ${lastJobStatus})`);
    }
    const remaining = deadline - now();
    if (remaining > 0) await sleep(Math.min(100, remaining));
  }
}
