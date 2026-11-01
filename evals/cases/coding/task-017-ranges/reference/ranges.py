def merge(ranges):
    merged = []
    for start, end in sorted(ranges):
        if start > end:
            raise ValueError("start must not exceed end")
        if merged and start <= merged[-1][1] + 1:
            merged[-1] = (merged[-1][0], max(merged[-1][1], end))
        else:
            merged.append((start, end))
    return merged
