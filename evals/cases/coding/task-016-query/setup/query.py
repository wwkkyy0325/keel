def parse_query(text):
    result = {}
    for pair in text.split("&"):
        if not pair:
            continue
        key, _, value = pair.partition("=")
        result[key] = value
    return result
