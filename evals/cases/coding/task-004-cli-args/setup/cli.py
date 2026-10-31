def parse_args(argv):
    result = {}
    index = 1
    while index < len(argv):
        key = argv[index].lstrip("-")
        result[key] = argv[index + 1]
        index += 2
    return result
