# Fix one-based pagination

`pagination.page(items, number, size)` uses one-based page numbers. It currently starts page 1 at index `size`. Fix the offset and reject page or size values below 1 with `ValueError`.

Do not change the tests. Run the test suite to confirm the fix.
