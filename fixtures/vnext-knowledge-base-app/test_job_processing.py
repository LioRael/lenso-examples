"""A Jobs claim must finish the exact new note despite DB clock skew or older work."""

import unittest
import urllib.error

from job_processing import process_queued_job


class FakeClock:
    def __init__(self):
        self.now = 0.0

    def monotonic(self):
        return self.now

    def sleep(self, seconds):
        self.now += seconds


class FakeJobsAPI:
    def __init__(self, clock, *, eligible_at=1.0, older_jobs=0):
        self.clock = clock
        self.eligible_at = eligible_at
        self.older_jobs = older_jobs
        self.completed = False
        self.process_calls = 0

    def http_json(self, url, method="GET", body=None, token=None):
        if url.endswith("/job-status/job-target"):
            return {
                "attempts": 1 if self.completed else 0,
                "jobId": "job-target",
                "status": "succeeded" if self.completed else "queued",
            }
        if url.endswith("/jobs/process-next"):
            assert method == "POST" and body == {} and token == "user-token"
            self.process_calls += 1
            if self.clock.now < self.eligible_at:
                raise urllib.error.HTTPError(url, 502, "Bad Gateway", {}, None)
            if self.older_jobs:
                self.older_jobs -= 1
            else:
                self.completed = True
            return {"processed": True}
        raise AssertionError(f"unexpected URL: {url}")


class JobProcessingTests(unittest.TestCase):
    def test_waits_for_exact_job_when_database_clock_is_behind(self):
        clock = FakeClock()
        api = FakeJobsAPI(clock)
        result = process_queued_job(
            "http://127.0.0.1:8000/", "job-target", "user-token", api.http_json,
            clock=clock.monotonic, sleep=clock.sleep,
        )
        self.assertEqual(result, {"attempts": 1, "jobId": "job-target", "status": "succeeded"})
        self.assertGreater(api.process_calls, 1)

    def test_ignores_success_for_an_older_queued_job(self):
        clock = FakeClock()
        api = FakeJobsAPI(clock, eligible_at=0, older_jobs=1)
        result = process_queued_job(
            "http://127.0.0.1:8000", "job-target", "user-token", api.http_json,
            clock=clock.monotonic, sleep=clock.sleep,
        )
        self.assertEqual(result["status"], "succeeded")
        self.assertEqual(api.process_calls, 2)

    def test_reports_a_claim_that_never_becomes_eligible(self):
        clock = FakeClock()
        api = FakeJobsAPI(clock, eligible_at=100)
        with self.assertRaisesRegex(TimeoutError, "never ready.*HTTP 502"):
            process_queued_job(
                "http://127.0.0.1:8000", "job-target", "user-token", api.http_json,
                timeout_seconds=0.25, clock=clock.monotonic, sleep=clock.sleep,
            )
        self.assertLessEqual(clock.now, 0.25)
        self.assertGreater(api.process_calls, 1)

    def test_does_not_retry_other_gateway_errors(self):
        clock = FakeClock()
        api = FakeJobsAPI(clock, eligible_at=100)

        def unavailable(url, **kwargs):
            if url.endswith("/jobs/process-next"):
                raise urllib.error.HTTPError(url, 503, "Unavailable", {}, None)
            return api.http_json(url, **kwargs)

        with self.assertRaises(urllib.error.HTTPError) as raised:
            process_queued_job(
                "http://127.0.0.1:8000", "job-target", "user-token", unavailable,
                clock=clock.monotonic, sleep=clock.sleep,
            )
        self.assertEqual(raised.exception.code, 503)
        raised.exception.close()
        self.assertEqual(clock.now, 0)

    def test_does_not_hide_a_job_that_was_claimed_but_not_completed(self):
        clock = FakeClock()
        claimed = False

        def failed_after_claim(url, **kwargs):
            nonlocal claimed
            if url.endswith("/job-status/job-target"):
                return {
                    "attempts": 1 if claimed else 0,
                    "jobId": "job-target",
                    "status": "running" if claimed else "queued",
                }
            if url.endswith("/jobs/process-next"):
                claimed = True
                raise urllib.error.HTTPError(url, 502, "Bad Gateway", {}, None)
            raise AssertionError(f"unexpected URL: {url}")

        with self.assertRaisesRegex(RuntimeError, "changed unexpectedly"):
            process_queued_job(
                "http://127.0.0.1:8000", "job-target", "user-token", failed_after_claim,
                clock=clock.monotonic, sleep=clock.sleep,
            )
        self.assertEqual(clock.now, 0.1)


if __name__ == "__main__":
    unittest.main()
