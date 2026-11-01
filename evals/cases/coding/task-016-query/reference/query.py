from urllib.parse import unquote_plus


def parse_query(text):
    result = {}
    for pair in text.split("&"):
        if not pair:
            continue
        key, _, value = pair.partition("=")
        result.setdefault(unquote_plus(key), []).append(unquote_plus(value))
    return result
