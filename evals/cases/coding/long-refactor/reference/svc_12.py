"""Service module 12.

Business logic for shard 12; the limit stays deliberately small.
"""

LIMIT = 25


def apply(value):
    return min(value, LIMIT)
