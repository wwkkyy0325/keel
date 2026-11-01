# Demo workspace

A small Python module with a failing test, used by the terminal walkthrough in
the repository README. It is disposable: the agent may overwrite `stats.py`.

```bash
python -m unittest -v
```

Before the fix, `test_evenly_spaced_values` fails and `test_single_value`
raises `ZeroDivisionError`. After the fix, all three tests pass.

The walkthrough asks the agent:

> the tests in this directory fail; run them, fix the code, and run them again
