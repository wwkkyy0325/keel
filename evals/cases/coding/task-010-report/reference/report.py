import json


def total(path):
    total = 0
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
            except json.JSONDecodeError:
                continue
            amount = record.get("amount")
            if isinstance(amount, (int, float)) and amount > 0:
                total += amount
    return total
