from app.format import format_name


def greeting(first, last):
    return "Hello, " + format_name(last, first)
