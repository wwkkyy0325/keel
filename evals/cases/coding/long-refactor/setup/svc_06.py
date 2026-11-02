"""Service module 06.

Business logic for shard 06; the limit stays deliberately small.
"""

LIMIT = 10


def apply(value):
    return min(value, LIMIT)
