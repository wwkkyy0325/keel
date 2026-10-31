def retry(operation, attempts):
    for _ in range(attempts):
        try:
            return operation()
        except Exception:
            pass
    raise RuntimeError("operation failed")
