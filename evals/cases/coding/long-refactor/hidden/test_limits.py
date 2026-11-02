import importlib
import unittest


class ServiceLimits(unittest.TestCase):
    def test_every_module_allows_twenty_five(self):
        for i in range(20):
            name = f"svc_{i:02d}"
            module = importlib.import_module(name)
            self.assertEqual(module.LIMIT, 25, name)
            self.assertEqual(module.apply(30), 25, name)

    def test_docstrings_survive(self):
        for i in range(20):
            module = importlib.import_module(f"svc_{i:02d}")
            self.assertTrue(module.__doc__, f"svc_{i:02d} lost its docstring")


if __name__ == "__main__":
    unittest.main()
