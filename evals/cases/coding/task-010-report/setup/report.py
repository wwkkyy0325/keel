import json


def total(path):
    total = 0
    with open(path) as handle:
        for line in handle:
            total += json.loads(line)["amount"]
    return total
