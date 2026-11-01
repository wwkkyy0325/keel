# Make the LRU cache evict the least recent key

`cache.LRUCache` stores values but never evicts and `get` does not refresh recency. Keep at most `capacity` keys, evict the least recently used (a read counts as use), and reject a capacity that is not a positive integer.

Do not change the tests. Run the test suite to confirm the fix.
