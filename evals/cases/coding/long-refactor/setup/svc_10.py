"""Service module 10.

Business logic for shard 10; the limit stays deliberately small.
"""

LIMIT = 10


def apply(value):
    return min(value, LIMIT)
