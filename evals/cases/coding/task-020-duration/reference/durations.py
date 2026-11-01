import re

UNITS = {"s": 1, "m": 60, "h": 3600}


def parse(text):
    if not isinstance(text, str) or not text.strip():
        raise ValueError("empty duration")
    compact = text.replace(" ", "").lower()
    total = 0
    seen = set()
    position = 0
    for match in re.finditer(r"(\d+)([smh])", compact):
        if match.start() != position:
            raise ValueError("invalid duration: " + text)
        unit = match.group(2)
        if unit in seen:
            raise ValueError("duplicate unit: " + unit)
        seen.add(unit)
        total += int(match.group(1)) * UNITS[unit]
        position = match.end()
    if position != len(compact):
        raise ValueError("invalid duration: " + text)
    return total
