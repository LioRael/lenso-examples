import assert from "node:assert/strict";
import { test } from "node:test";
import { create } from "./project/app/state/src/host_facilities/state.mjs";

test("a committed D1 batch with a lost receipt response is unknown and can be queried", async () => {
  let batches = 0;
  let loseResponse = true;
  let stored;
  const database = {
    prepare(query) {
      return {
        bind(...values) { this.values = values; return this; },
        async first() {
          if (query.includes("intent_digest") && loseResponse) {
            loseResponse = false;
            throw new Error("receipt response lost after commit");
          }
          const { intent_digest: digest, ...receipt } = stored;
          return query.includes("intent_digest") ? stored : receipt;
        },
      };
    },
    async batch(statements) {
      batches += 1;
      const [key, digest, value] = statements[0].values;
      stored = { idempotency_key: key, intent_digest: digest, label: "primary-state", value,
        revision: 1, receipt_id: "primary-state:1:" + digest };
    },
  };
  const handle = create(database, { run: (work) => work() }, { profile: "workers-d1" });
  const request = { value: 47, expected_revision: 0, idempotency_key: "write-1" };
  assert.deepEqual(await handle.update(request), { error: "unknown_commit" });
  assert.equal(batches, 1);
  const query = await handle.receipt({ idempotency_key: "write-1" });
  assert.equal(query.ok.found, true);
  assert.equal(query.ok.value, 47);
  assert.equal(query.ok.revision, 1);
  assert.equal(batches, 1);
});

test("a failed D1 batch response remains unknown without a retry", async () => {
  let batches = 0;
  const database = {
    prepare() { return { bind() { return this; } }; },
    async batch() { batches += 1; throw new Error("batch response lost"); },
  };
  const handle = create(database, { run: (work) => work() }, { profile: "workers-d1" });
  assert.deepEqual(await handle.update({ value: 47, expected_revision: 0, idempotency_key: "write-1" }),
    { error: "unknown_commit" });
  assert.equal(batches, 1);
});
