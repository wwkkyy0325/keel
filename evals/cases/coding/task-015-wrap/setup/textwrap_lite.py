def wrap(text, width):
    if width <= 0:
        raise ValueError("width must be positive")
    return "\n".join(
        text[index : index + width] for index in range(0, len(text), width)
    )
