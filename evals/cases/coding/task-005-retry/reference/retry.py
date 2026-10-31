def retry(operation, attempts, retry_on=(ValueError,)):
    if attempts < 1:
        raise ValueError("attempts must be positive")
    for attempt in range(attempts):
        try:
            return operation()
        except retry_on:
            if attempt == attempts - 1:
                raise
    raise AssertionError("unreachable")
