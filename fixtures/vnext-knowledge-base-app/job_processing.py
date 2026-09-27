"""Observe one background job and its note through the public HTTP boundary."""

import time


def wait_for_processed_note(
    base_url, note_id, job_id, token, http_json, *,
    timeout_seconds=10, clock=time.monotonic, sleep=time.sleep,
):
    """Wait for a persisted note and its durable job, or inline completion."""
    base_url = base_url.rstrip("/")
    note_url = base_url + "/notes/" + note_id
    deadline = clock() + timeout_seconds
    last_job_status = "awaiting dispatch"
    if job_id is not None and (not isinstance(job_id, str) or not job_id):
        raise RuntimeError(f"note {note_id} has an invalid initial job ID")

    while True:
        note = http_json(note_url, token=token)
        if note.get("id") != note_id or (job_id is not None and note.get("job_id") != job_id):
            raise RuntimeError(f"read back another note or job for {note_id}: {note!r}")
        if "job_id" not in note or (
            note["job_id"] is not None
            and (not isinstance(note["job_id"], str) or not note["job_id"])
        ):
            raise RuntimeError(f"note {note_id} has an invalid job ID")
        job_id = note.get("job_id")
        if note.get("processing_status") == "failed":
            raise RuntimeError(f"excerpt processing failed for note {note_id} (job {last_job_status})")
        if note.get("processing_status") not in {"dispatch_pending", "queued", "succeeded"}:
            raise RuntimeError(f"unexpected processing status for note {note_id}: {note!r}")
        if job_id is not None:
            if job_id.startswith("inline:") and note.get("processing_status") == "succeeded":
                return {"attempts": 0, "jobId": job_id, "status": "succeeded"}, note
            job = http_json(base_url + "/job-status/" + job_id, token=token)
            if job.get("jobId") != job_id:
                raise RuntimeError(f"Jobs returned another job for {job_id}: {job!r}")
            last_job_status = job.get("status")
            if last_job_status not in {"queued", "running", "succeeded", "failed"}:
                raise RuntimeError(f"unexpected job status for {job_id}: {job!r}")
            if note.get("processing_status") == "succeeded":
                if last_job_status != "succeeded":
                    raise RuntimeError(f"note {note_id} succeeded before its job completed")
                return job, note
        elif note.get("processing_status") == "succeeded":
            raise RuntimeError(f"note {note_id} succeeded without a job ID")
        if clock() >= deadline:
            raise TimeoutError(
                f"note {note_id} did not finish before the deadline "
                f"(job {last_job_status})"
            )

        remaining = deadline - clock()
        if remaining > 0:
            sleep(min(0.1, remaining))
