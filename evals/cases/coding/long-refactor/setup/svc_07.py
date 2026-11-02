"""Service module 07.

Business logic for shard 07; the limit stays deliberately small.
"""

LIMIT = 10


def apply(value):
    return min(value, LIMIT)
