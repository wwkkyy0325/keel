Every service module at the workspace root (`svc_00.py` through
`svc_19.py`) still applies the conservative limit of 10. Raise all twenty
modules to `LIMIT = 25` so that `apply(30)` returns 25.

Rules:
- change only the limit; keep the docstrings and the `apply` function as they
  are;
- do not change `check.py` or any test;
- `python check.py` must pass when you are done;
- work through the whole list in one turn.
