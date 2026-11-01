def parse(text):
    unit = text[-1]
    return int(text[:-1]) * {"s": 1, "m": 60, "h": 3600}[unit]
