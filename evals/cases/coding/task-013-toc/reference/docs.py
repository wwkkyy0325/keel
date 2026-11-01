def toc(markdown):
    entries = []
    fenced = False
    for line in markdown.splitlines():
        if line.strip().startswith("```"):
            fenced = not fenced
            continue
        if fenced:
            continue
        level = 0
        while level < len(line) and line[level] == "#":
            level += 1
        if 1 <= level <= 3 and level < len(line) and line[level] == " ":
            title = line[level:].strip()
            if title:
                entries.append((level, title))
    return entries
