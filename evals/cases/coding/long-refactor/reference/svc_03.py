"""Service module 03.

Business logic for shard 03; the limit stays deliberately small.
"""

LIMIT = 25


def apply(value):
    return min(value, LIMIT)
