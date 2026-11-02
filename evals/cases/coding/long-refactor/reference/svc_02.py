"""Service module 02.

Business logic for shard 02; the limit stays deliberately small.
"""

LIMIT = 25


def apply(value):
    return min(value, LIMIT)
