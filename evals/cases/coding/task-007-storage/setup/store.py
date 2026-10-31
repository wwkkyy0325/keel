import json


def load(path):
    with open(path) as handle:
        return json.load(handle)


def save(path, value):
    with open(path, "w") as handle:
        json.dump(value, handle)
