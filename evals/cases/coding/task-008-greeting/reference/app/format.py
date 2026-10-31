def format_name(first, last):
    parts = [part.strip() for part in (first, last) if part and part.strip()]
    return " ".join(parts)
