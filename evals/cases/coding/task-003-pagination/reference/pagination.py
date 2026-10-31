def page(items, number, size):
    if number < 1 or size < 1:
        raise ValueError("page and size must be positive")
    start = (number - 1) * size
    return items[start:start + size]
