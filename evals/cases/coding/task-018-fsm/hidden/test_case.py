import unittest

from machine import run


TRANSITIONS = {
    "idle": {"start": "running"},
    "running": {"stop": "idle"},
}


class MachineTests(unittest.TestCase):
    def test_sequence(self):
        self.assertEqual(run(TRANSITIONS, "idle", ["start"]), "running")
        self.assertEqual(run(TRANSITIONS, "idle", ["start", "stop"]), "idle")

    def test_empty_events(self):
        self.assertEqual(run(TRANSITIONS, "idle", []), "idle")

    def test_unknown_event(self):
        with self.assertRaises(ValueError):
            run(TRANSITIONS, "running", ["start"])

    def test_unknown_start(self):
        with self.assertRaises(ValueError):
            run(TRANSITIONS, "done", [])


if __name__ == "__main__":
    unittest.main()
