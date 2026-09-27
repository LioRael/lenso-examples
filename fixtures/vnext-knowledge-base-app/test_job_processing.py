"""Read-only polling of a durable background job and its stored note."""

import unittest

from job_processing import wait_for_processed_note


class FakeClock:
    def __init__(self):
        self.now = 0.0

    def monotonic(self):
        return self.now

    def sleep(self, seconds):
        self.now += seconds


class FakeJobsAPI:
    def __init__(self, clock, *, dispatch_ready_at=0, job_ready_at=0.2, note_ready_at=0.3):
        self.clock = clock
        self.dispatch_ready_at = dispatch_ready_at
        self.job_ready_at = job_ready_at
        self.note_ready_at = note_ready_at
        self.requests = []

    def http_json(self, url, method="GET", body=None, token=None):
        self.requests.append((url, method, body, token))
        if url.endswith("/job-status/job-target"):
            completed = self.clock.now >= self.job_ready_at
            return {
                "attempts": 1 if completed else 0,
                "jobId": "job-target",
                "status": "succeeded" if completed else "queued",
            }
        if url.endswith("/notes/note-target"):
            if self.clock.now < self.dispatch_ready_at:
                return {
                    "id": "note-target", "job_id": None,
                    "processing_status": "dispatch_pending",
                }
            completed = self.clock.now >= self.note_ready_at
            return {
                "id": "note-target",
                "job_id": "job-target",
                "processing_status": "succeeded" if completed else "queued",
            }
        raise AssertionError(f"unexpected URL: {url}")


class JobProcessingTests(unittest.TestCase):
    def test_waits_for_job_and_note_without_claiming_work(self):
        clock = FakeClock()
        api = FakeJobsAPI(clock)
        job, note = wait_for_processed_note(
            "http://127.0.0.1:8000/", "note-target", "job-target",
            "user-token", api.http_json, clock=clock.monotonic, sleep=clock.sleep,
        )
        self.assertEqual(job, {"attempts": 1, "jobId": "job-target", "status": "succeeded"})
        self.assertEqual(note["processing_status"], "succeeded")
        self.assertGreaterEqual(clock.now, 0.3)
        self.assertTrue(all(method == "GET" and body is None and token == "user-token"
                            for _, method, body, token in api.requests))

    def test_waits_for_dispatch_before_inspecting_a_real_job(self):
        clock = FakeClock()
        api = FakeJobsAPI(clock, dispatch_ready_at=0.2)
        job, note = wait_for_processed_note(
            "http://127.0.0.1:8000", "note-target", None,
            "user-token", api.http_json, clock=clock.monotonic, sleep=clock.sleep,
        )
        self.assertEqual(job["status"], "succeeded")
        self.assertEqual(note["processing_status"], "succeeded")
        first_job_request = next(index for index, request in enumerate(api.requests)
                                 if "/job-status/" in request[0])
        self.assertGreaterEqual(first_job_request, 3)
        self.assertTrue(all("/notes/note-target" in request[0]
                            for request in api.requests[:first_job_request]))
        self.assertFalse(any("/job-status/None" in request[0] for request in api.requests))

    def test_dispatch_pending_times_out_without_job_inspection(self):
        clock = FakeClock()
        api = FakeJobsAPI(clock, dispatch_ready_at=100)
        with self.assertRaisesRegex(TimeoutError, "did not finish.*awaiting dispatch"):
            wait_for_processed_note(
                "http://127.0.0.1:8000", "note-target", None,
                "user-token", api.http_json, timeout_seconds=0.25,
                clock=clock.monotonic, sleep=clock.sleep,
            )
        self.assertTrue(all("/notes/note-target" in request[0] for request in api.requests))

    def test_inline_completion_after_dispatch_pending_never_inspects_jobs(self):
        clock = FakeClock()
        requests = []

        def inline(url, method="GET", body=None, token=None):
            requests.append((url, method, body, token))
            self.assertTrue(url.endswith("/notes/note-target"))
            if len(requests) == 1:
                return {"id": "note-target", "job_id": None, "processing_status": "dispatch_pending"}
            return {"id": "note-target", "job_id": "inline:note-target", "processing_status": "succeeded"}

        job, note = wait_for_processed_note(
            "http://127.0.0.1:8000", "note-target", None,
            "user-token", inline, clock=clock.monotonic, sleep=clock.sleep,
        )
        self.assertEqual(job, {"attempts": 0, "jobId": "inline:note-target", "status": "succeeded"})
        self.assertEqual(note["processing_status"], "succeeded")
        self.assertEqual(len(requests), 2)
        self.assertTrue(all(method == "GET" and body is None and token == "user-token"
                            for _, method, body, token in requests))

    def test_waits_when_job_succeeds_before_note_is_committed(self):
        clock = FakeClock()
        api = FakeJobsAPI(clock, job_ready_at=0, note_ready_at=0.2)
        _, note = wait_for_processed_note(
            "http://127.0.0.1:8000", "note-target", "job-target",
            "user-token", api.http_json, clock=clock.monotonic, sleep=clock.sleep,
        )
        self.assertEqual(note["processing_status"], "succeeded")
        self.assertGreaterEqual(clock.now, 0.2)

    def test_reports_worker_that_does_not_complete(self):
        clock = FakeClock()
        api = FakeJobsAPI(clock, job_ready_at=100, note_ready_at=100)
        with self.assertRaisesRegex(TimeoutError, "did not finish.*job queued"):
            wait_for_processed_note(
                "http://127.0.0.1:8000", "note-target", "job-target",
                "user-token", api.http_json, timeout_seconds=0.25,
                clock=clock.monotonic, sleep=clock.sleep,
            )
        self.assertLessEqual(clock.now, 0.25)

    def test_failed_note_is_not_presented_as_success(self):
        clock = FakeClock()

        def failed(url, method="GET", body=None, token=None):
            if url.endswith("/job-status/job-target"):
                return {"attempts": 1, "jobId": "job-target", "status": "failed"}
            return {"id": "note-target", "job_id": "job-target", "processing_status": "failed"}

        with self.assertRaisesRegex(RuntimeError, "excerpt processing failed"):
            wait_for_processed_note(
                "http://127.0.0.1:8000", "note-target", "job-target",
                "user-token", failed, clock=clock.monotonic, sleep=clock.sleep,
            )

    def test_rejects_another_job_or_note(self):
        clock = FakeClock()
        api = FakeJobsAPI(clock, job_ready_at=0, note_ready_at=0)

        def wrong_job(url, **kwargs):
            result = api.http_json(url, **kwargs)
            if url.endswith("/job-status/job-target"):
                result["jobId"] = "job-other"
            return result

        with self.assertRaisesRegex(RuntimeError, "another job"):
            wait_for_processed_note(
                "http://127.0.0.1:8000", "note-target", "job-target",
                "user-token", wrong_job, clock=clock.monotonic, sleep=clock.sleep,
            )

        def wrong_note(url, **kwargs):
            result = api.http_json(url, **kwargs)
            if url.endswith("/notes/note-target"):
                result["id"] = "note-other"
            return result

        with self.assertRaisesRegex(RuntimeError, "another note or job"):
            wait_for_processed_note(
                "http://127.0.0.1:8000", "note-target", "job-target",
                "user-token", wrong_note, clock=clock.monotonic, sleep=clock.sleep,
            )


if __name__ == "__main__":
    unittest.main()
