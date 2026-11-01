def parse(text):
    rows = []
    row = []
    field = []
    quoted = False
    index = 0
    while index < len(text):
        char = text[index]
        if quoted:
            if char == '"':
                if index + 1 < len(text) and text[index + 1] == '"':
                    field.append('"')
                    index += 1
                else:
                    quoted = False
            else:
                field.append(char)
        elif char == '"':
            quoted = True
        elif char == ",":
            row.append("".join(field))
            field = []
        elif char in "\r\n":
            if char == "\r" and index + 1 < len(text) and text[index + 1] == "\n":
                index += 1
            row.append("".join(field))
            field = []
            rows.append(row)
            row = []
        else:
            field.append(char)
        index += 1
    if field or row:
        row.append("".join(field))
        rows.append(row)
    return rows
