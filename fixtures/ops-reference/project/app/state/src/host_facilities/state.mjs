const keyValid = (value) => typeof value === "string" && /^[A-Za-z0-9._-]{1,128}$/.test(value);
const fail = (error) => ({ error });
const snapshot = (row) => ({ label: row.label, value: row.value, revision: row.revision });

async function intent(request) {
  const bytes = new TextEncoder().encode(`ops-state.v1\n${request.expected_revision}\n${request.value}`);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

// The Host supplies one selected D1 binding and its request scope, never the full env.
export function create(database, scope, configuration) {
  if (configuration?.profile === "simulated") return Object.freeze({ profile: "simulated" });
  if (configuration?.profile !== "workers-d1" || typeof database?.prepare !== "function"
      || typeof database?.batch !== "function" || typeof scope?.run !== "function") {
    throw new Error("invalid_ops_state_facility");
  }
  const execute = (work) => scope.run(async () => {
    try { return await work(); } catch { return fail("unavailable"); }
  });
  const read = async () => {
    const version = await database.prepare("SELECT version FROM schema_version WHERE singleton = 1").first();
    if (version?.version !== 1) return fail("setup_required");
    const row = await database.prepare("SELECT label, value, revision FROM state WHERE singleton = 1").first();
    return row ? { ok: snapshot(row) } : fail("setup_required");
  };
  return Object.freeze({
    profile: "workers-d1",
    read() { return execute(read); },
    receipt(request) {
      if (!keyValid(request.idempotency_key)) return Promise.resolve(fail("invalid_input"));
      return execute(async () => {
        const row = await database.prepare("SELECT label,value,revision,receipt_id FROM receipts WHERE idempotency_key = ?")
          .bind(request.idempotency_key).first();
        return { ok: row ? { found: true, ...snapshot(row), receipt_id: row.receipt_id }
          : { found: false, label: null, value: null, revision: null, receipt_id: null } };
      });
    },
    update(request) {
      if (!Number.isSafeInteger(request.value) || !Number.isSafeInteger(request.expected_revision)
          || request.expected_revision < 0 || request.expected_revision >= Number.MAX_SAFE_INTEGER
          || !keyValid(request.idempotency_key)) {
        return Promise.resolve(fail("invalid_input"));
      }
      return execute(async () => {
        const digest = await intent(request);
        // D1 batch is one transaction. Its conditional INSERT and UPDATE share a CAS;
        // neither a replay nor a competing batch can increment the state twice.
        try { await database.batch([
          database.prepare(`INSERT INTO receipts (idempotency_key,intent_digest,label,value,revision,receipt_id)
            SELECT ?,?,label,?,revision+1,label || ':' || (revision+1) || ':' || ? FROM state
            WHERE singleton=1 AND revision=? ON CONFLICT(idempotency_key) DO NOTHING`)
            .bind(request.idempotency_key, digest, request.value, digest, request.expected_revision),
          database.prepare(`UPDATE state SET value=?, revision=revision+1 WHERE singleton=1 AND revision=?
            AND EXISTS(SELECT 1 FROM receipts WHERE idempotency_key=? AND intent_digest=? AND revision=state.revision+1)`)
            .bind(request.value, request.expected_revision, request.idempotency_key, digest),
        ]); } catch { return fail("unknown_commit"); }
        let row;
        try {
          row = await database.prepare("SELECT intent_digest,label,value,revision,receipt_id FROM receipts WHERE idempotency_key=?")
            .bind(request.idempotency_key).first();
        } catch { return fail("unknown_commit"); }
        if (!row) return fail("stale_revision");
        if (row.intent_digest !== digest) return fail("idempotency_conflict");
        return { ok: { ...snapshot(row), receipt_id: row.receipt_id } };
      });
    },
  });
}
