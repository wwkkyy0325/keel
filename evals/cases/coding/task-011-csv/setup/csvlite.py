def parse(text):
    rows = []
    for line in text.splitlines():
        if line.strip():
            rows.append(line.split(","))
    return rows
