import json
import os
import tempfile
import unittest

from report import total


class ReportTests(unittest.TestCase):
    def _write(self, lines):
        handle, path = tempfile.mkstemp(suffix=".jsonl")
        os.close(handle)
        with open(path, "w", encoding="utf-8") as stream:
            for line in lines:
                stream.write(line + "\n")
        return path

    def test_skips_bad_lines_and_negative(self):
        path = self._write(
            [
                json.dumps({"amount": 3}),
                "",
                "not json",
                json.dumps({"amount": -5}),
                json.dumps({"amount": "4"}),
                json.dumps({"amount": 2}),
                json.dumps({"other": 9}),
            ]
        )
        try:
            self.assertEqual(total(path), 5)
        finally:
            os.unlink(path)

    def test_empty_file(self):
        path = self._write([])
        try:
            self.assertEqual(total(path), 0)
        finally:
            os.unlink(path)


if __name__ == "__main__":
    unittest.main()
