import assert from 'node:assert/strict';
import test from 'node:test';
import { processQueuedJob } from './src/process-job.ts';

test('a briefly unavailable claim still completes the new note job', async () => {
  let now = 0;
  let claimCalls = 0;
  let status = 'queued';
  const result = await processQueuedJob('job-target', {
    inspect: async () => ({ attempts: status === 'succeeded' ? 1 : 0, jobId: 'job-target', status }),
    claim: async () => {
      claimCalls += 1;
      if (claimCalls === 1) throw Object.assign(new Error('Bad Gateway'), { response: { status: 502 } });
      status = 'succeeded';
      return { processed: true };
    },
    isRetryableClaimError: (error) => error?.response?.status === 502,
  }, {
    now: () => now,
    sleep: async (milliseconds) => { now += milliseconds; },
  });
  assert.deepEqual(result, { attempts: 1, jobId: 'job-target', status: 'succeeded' });
});

test('claiming older work does not complete the new note job', async () => {
  let now = 0;
  let claims = 0;
  const result = await processQueuedJob('job-target', {
    inspect: async () => ({
      attempts: claims === 2 ? 1 : 0,
      jobId: 'job-target',
      status: claims === 2 ? 'succeeded' : 'queued',
    }),
    claim: async () => { claims += 1; return { processed: true }; },
    isRetryableClaimError: () => false,
  }, {
    now: () => now,
    sleep: async (milliseconds) => { now += milliseconds; },
  });
  assert.deepEqual(result, { attempts: 1, jobId: 'job-target', status: 'succeeded' });
  assert.equal(claims, 2);
});

test('a persistently unavailable claim stops at the deadline', async () => {
  let now = 0;
  await assert.rejects(processQueuedJob('job-target', {
    inspect: async () => ({ attempts: 0, jobId: 'job-target', status: 'queued' }),
    claim: async () => { throw Object.assign(new Error('Bad Gateway'), { response: { status: 502 } }); },
    isRetryableClaimError: (error) => error?.response?.status === 502,
  }, {
    now: () => now,
    sleep: async (milliseconds) => { now += milliseconds; },
    timeoutMs: 250,
  }), /never ready \(last claim returned HTTP 502\)/);
  assert.equal(now, 250);
});

test('a claimed job in another state is not retried', async () => {
  let now = 0;
  let inspected = false;
  await assert.rejects(processQueuedJob('job-target', {
    inspect: async () => {
      if (inspected) return { attempts: 1, jobId: 'job-target', status: 'running' };
      inspected = true;
      return { attempts: 0, jobId: 'job-target', status: 'queued' };
    },
    claim: async () => { throw Object.assign(new Error('Bad Gateway'), { response: { status: 502 } }); },
    isRetryableClaimError: (error) => error?.response?.status === 502,
  }, {
    now: () => now,
    sleep: async (milliseconds) => { now += milliseconds; },
  }), /changed unexpectedly to running/);
});
