"""Keep browser-mutated excerpt policy expectations aligned with the JS Plugin."""

import unittest

from excerpt_expectations import expected_excerpt


class ExcerptExpectationsTests(unittest.TestCase):
    def test_browser_policy_change_updates_later_inline_expectation(self):
        body = "Jobs is disabled, so deterministic processing completes inline."
        self.assertEqual(expected_excerpt(body, 48), body[:47] + "…")
        self.assertEqual(expected_excerpt(body, 64), body)

    def test_normalizes_javascript_whitespace_and_counts_unicode_characters(self):
        self.assertEqual(expected_excerpt("\u00a0 A\u3000😀B\ufeff", 3), "A …")


if __name__ == "__main__":
    unittest.main()
