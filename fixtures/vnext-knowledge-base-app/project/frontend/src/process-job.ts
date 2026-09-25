export type JobState = { attempts: number; jobId: string; status: string };

export type JobOperations = {
  inspect: (jobId: string) => Promise<JobState>;
  claim: () => Promise<{ processed: boolean }>;
  isRetryableClaimError: (error: unknown) => boolean;
};

export type JobTiming = {
  now?: () => number;
  sleep?: (milliseconds: number) => Promise<void>;
  timeoutMs?: number;
};

export async function processQueuedJob(
  jobId: string,
  operations: JobOperations,
  timing: JobTiming = {},
): Promise<JobState> {
  const now = timing.now ?? (() => performance.now());
  const sleep = timing.sleep ?? ((milliseconds: number) => new Promise<void>((resolve) => {
    setTimeout(resolve, milliseconds);
  }));
  const deadline = now() + (timing.timeoutMs ?? 5_000);
  let lastClaim = 'not attempted';

  while (true) {
    const status = await operations.inspect(jobId);
    if (status.jobId !== jobId) throw new Error(`Jobs returned another job for ${jobId}`);
    if (status.status === 'succeeded' && status.attempts === 1) return status;
    if (status.status !== 'queued' || status.attempts !== 0) {
      throw new Error(`Target job ${jobId} changed unexpectedly to ${status.status} (attempts ${status.attempts})`);
    }
    if (now() >= deadline) throw new Error(`Target job ${jobId} was never ready (${lastClaim})`);

    try {
      const result = await operations.claim();
      if (!result.processed) throw new Error('Jobs did not process queued work');
      lastClaim = 'last claim processed another queued job';
    } catch (error) {
      if (!operations.isRetryableClaimError(error)) throw error;
      lastClaim = 'last claim returned HTTP 502';
    }

    const remaining = deadline - now();
    if (remaining > 0) await sleep(Math.min(100, remaining));
  }
}
