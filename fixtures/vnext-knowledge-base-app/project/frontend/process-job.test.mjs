import assert from 'node:assert/strict';
import test from 'node:test';
import { waitForProcessedNote } from './src/process-job.ts';

const queued = { id: 'note-target', job_id: 'job-target', processing_status: 'queued' };
const succeeded = { ...queued, processing_status: 'succeeded' };
const dispatchPending = { ...queued, job_id: null, processing_status: 'dispatch_pending' };
const inlineSucceeded = { ...succeeded, job_id: 'inline:note-target' };

test('reads only job and note status until the worker completes both', async () => {
  let now = 0;
  let polls = 0;
  let reads = 0;
  const operations = {
    inspect: async () => {
      polls += 1;
      return { attempts: 1, jobId: 'job-target', status: polls === 1 ? 'running' : 'succeeded' };
    },
    readNote: async () => ++reads === 3 ? succeeded : queued,
  };
  const result = await waitForProcessedNote('note-target', 'job-target', operations, {
    now: () => now,
    sleep: async (milliseconds) => { now += milliseconds; },
  });
  assert.deepEqual(result, succeeded);
  assert.equal(polls, 3);
});

test('waits for an assigned job ID before inspecting Jobs after a 202 response', async () => {
  let now = 0;
  let reads = 0;
  let inspections = 0;
  const result = await waitForProcessedNote('note-target', null, {
    readNote: async () => {
      reads += 1;
      if (reads <= 2) return dispatchPending;
      return reads === 3 ? queued : succeeded;
    },
    inspect: async (jobId) => {
      assert.equal(jobId, 'job-target');
      assert.ok(reads >= 3);
      inspections += 1;
      return { attempts: 1, jobId, status: inspections === 1 ? 'running' : 'succeeded' };
    },
  }, {
    now: () => now,
    sleep: async (milliseconds) => { now += milliseconds; },
  });
  assert.deepEqual(result, succeeded);
  assert.equal(inspections, 2);
});

test('accepts inline completion after a 202 response without inspecting Jobs', async () => {
  let now = 0;
  let reads = 0;
  const result = await waitForProcessedNote('note-target', null, {
    readNote: async () => ++reads === 1 ? dispatchPending : inlineSucceeded,
    inspect: async () => { throw new Error('inline work has no Jobs record'); },
  }, {
    now: () => now,
    sleep: async (milliseconds) => { now += milliseconds; },
  });
  assert.deepEqual(result, inlineSucceeded);
  assert.equal(reads, 2);
});

test('keeps polling after Jobs succeeds until the note status catches up', async () => {
  let now = 0;
  let reads = 0;
  const result = await waitForProcessedNote('note-target', 'job-target', {
    inspect: async () => ({ attempts: 1, jobId: 'job-target', status: 'succeeded' }),
    readNote: async () => (++reads === 2 ? succeeded : queued),
  }, {
    now: () => now,
    sleep: async (milliseconds) => { now += milliseconds; },
  });
  assert.deepEqual(result, succeeded);
  assert.equal(reads, 2);
});

test('reports failed processing rather than treating a completed HTTP read as success', async () => {
  await assert.rejects(waitForProcessedNote('note-target', 'job-target', {
    inspect: async () => ({ attempts: 1, jobId: 'job-target', status: 'failed' }),
    readNote: async () => ({ ...queued, processing_status: 'failed' }),
  }), /Excerpt processing failed/);
});

test('a queued note stops at the deadline without invoking a processing endpoint', async () => {
  let now = 0;
  const result = waitForProcessedNote('note-target', 'job-target', {
    inspect: async () => ({ attempts: 0, jobId: 'job-target', status: 'queued' }),
    readNote: async () => queued,
  }, {
    now: () => now,
    sleep: async (milliseconds) => { now += milliseconds; },
    timeoutMs: 250,
  });
  await assert.rejects(result, /did not finish before the deadline \(job queued\)/);
  assert.equal(now, 250);
});

test('dispatch pending times out without querying an absent job ID', async () => {
  let now = 0;
  let reads = 0;
  await assert.rejects(waitForProcessedNote('note-target', null, {
    readNote: async () => { reads += 1; return dispatchPending; },
    inspect: async () => { throw new Error('must not inspect without a job ID'); },
  }, {
    now: () => now,
    sleep: async (milliseconds) => { now += milliseconds; },
    timeoutMs: 250,
  }), /did not finish before the deadline \(job awaiting dispatch\)/);
  assert.ok(reads > 1);
  assert.equal(now, 250);
});

test('rejects a mismatched job or note', async () => {
  await assert.rejects(waitForProcessedNote('note-target', 'job-target', {
    inspect: async () => ({ attempts: 1, jobId: 'job-other', status: 'succeeded' }),
    readNote: async () => succeeded,
  }), /another job/);
  await assert.rejects(waitForProcessedNote('note-target', 'job-target', {
    inspect: async () => ({ attempts: 1, jobId: 'job-target', status: 'succeeded' }),
    readNote: async () => ({ ...succeeded, id: 'note-other' }),
  }), /another note or job/);
});
