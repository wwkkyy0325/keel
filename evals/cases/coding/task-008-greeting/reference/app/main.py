from app.format import format_name


def greeting(first, last):
    name = format_name(first, last)
    return f"Hello, {name}!" if name else "Hello!"
