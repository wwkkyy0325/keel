def toc(markdown):
    entries = []
    for line in markdown.splitlines():
        if line.startswith("#"):
            level = len(line) - len(line.lstrip("#"))
            entries.append((level, line[level:].strip()))
    return entries
