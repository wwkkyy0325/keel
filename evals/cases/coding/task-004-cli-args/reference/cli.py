def parse_args(argv):
    result = {}
    index = 1
    while index < len(argv):
        arg = argv[index]
        if not arg.startswith("--") or arg == "--":
            raise ValueError(f"unexpected argument: {arg}")
        key = arg[2:].replace("-", "_")
        if index + 1 >= len(argv) or argv[index + 1].startswith("--"):
            raise ValueError(f"missing value for {arg}")
        result[key] = argv[index + 1]
        index += 2
    return result
