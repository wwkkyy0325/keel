"""Service module 05.

Business logic for shard 05; the limit stays deliberately small.
"""

LIMIT = 25


def apply(value):
    return min(value, LIMIT)
