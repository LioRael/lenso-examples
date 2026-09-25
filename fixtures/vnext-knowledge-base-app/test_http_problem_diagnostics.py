import io
import unittest
import urllib.error
from contextlib import redirect_stderr

from http_problem_diagnostics import report_http_server_error


class HttpProblemDiagnosticsTests(unittest.TestCase):
    def test_server_error_reports_only_status_and_validated_code(self):
        error = urllib.error.HTTPError(
            "https://example.invalid/private?token=secret", 503, "secret", {},
            io.BytesIO(b'{"code":"knowledge_storage_unavailable","detail":"secret"}'),
        )
        self.addCleanup(error.close)
        output = io.StringIO()
        with redirect_stderr(output):
            report_http_server_error(error)
        self.assertEqual(output.getvalue(), "HTTP 503 Problem code=knowledge_storage_unavailable\n")

    def test_untrusted_code_and_body_are_not_logged(self):
        error = urllib.error.HTTPError(
            "https://example.invalid/secret", 502, "secret", {},
            io.BytesIO(b'{"code":"secret\\nvalue","detail":"secret"}'),
        )
        self.addCleanup(error.close)
        output = io.StringIO()
        with redirect_stderr(output):
            report_http_server_error(error)
        self.assertEqual(output.getvalue(), "HTTP 502 Problem code=unavailable\n")

    def test_expected_client_error_remains_unread_and_unreported(self):
        body = io.BytesIO(b'{"detail":"secret"}')
        error = urllib.error.HTTPError("https://example.invalid/secret", 409, "secret", {}, body)
        self.addCleanup(error.close)
        output = io.StringIO()
        with redirect_stderr(output):
            report_http_server_error(error)
        self.assertEqual(output.getvalue(), "")
        self.assertEqual(body.tell(), 0)


if __name__ == "__main__":
    unittest.main()
