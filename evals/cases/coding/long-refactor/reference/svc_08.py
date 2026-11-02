"""Service module 08.

Business logic for shard 08; the limit stays deliberately small.
"""

LIMIT = 25


def apply(value):
    return min(value, LIMIT)
