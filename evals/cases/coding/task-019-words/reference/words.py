import re
from collections import Counter


def top(text, count):
    if count <= 0:
        return []
    counts = Counter(re.findall(r"[a-z0-9']+", text.lower()))
    return sorted(counts.items(), key=lambda item: (-item[1], item[0]))[:count]
