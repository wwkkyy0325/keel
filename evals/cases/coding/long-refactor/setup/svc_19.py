"""Service module 19.

Business logic for shard 19; the limit stays deliberately small.
"""

LIMIT = 10


def apply(value):
    return min(value, LIMIT)
