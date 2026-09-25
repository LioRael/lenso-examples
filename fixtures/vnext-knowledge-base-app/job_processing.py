"""Drive one queued reference job through the local App HTTP boundary."""

import time
import urllib.error


def process_queued_job(
    base_url, job_id, token, http_json, *,
    timeout_seconds=5, clock=time.monotonic, sleep=time.sleep,
):
    """Wait for the new job, not merely for any successful queue claim."""
    base_url = base_url.rstrip("/")
    status_url = base_url + "/job-status/" + job_id
    process_url = base_url + "/jobs/process-next"
    deadline = clock() + timeout_seconds
    last_claim = "not attempted"

    while True:
        status = http_json(status_url, token=token)
        if status.get("jobId") != job_id:
            raise RuntimeError(f"Jobs returned another job for {job_id}: {status!r}")
        if status.get("status") == "succeeded" and status.get("attempts") == 1:
            return status
        if status.get("status") != "queued" or status.get("attempts") != 0:
            raise RuntimeError(f"target job {job_id} changed unexpectedly: {status!r}")
        if clock() >= deadline:
            raise TimeoutError(f"target job {job_id} was never ready ({last_claim})")

        try:
            processed = http_json(process_url, method="POST", body={}, token=token)
        except urllib.error.HTTPError as error:
            if error.code != 502:
                raise
            error.close()
            last_claim = "last claim returned HTTP 502"
        else:
            if processed != {"processed": True}:
                raise RuntimeError(f"unexpected Jobs process-next response: {processed!r}")
            last_claim = "last claim processed another queued job"

        remaining = deadline - clock()
        if remaining > 0:
            sleep(min(0.1, remaining))
