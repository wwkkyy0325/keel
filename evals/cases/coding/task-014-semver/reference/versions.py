def compare(left, right):
    def parts(version):
        values = []
        for piece in version.split("."):
            digits = "".join(character for character in piece if character.isdigit())
            values.append(int(digits) if digits else 0)
        while len(values) < 3:
            values.append(0)
        return values[:3]

    left_parts = parts(left)
    right_parts = parts(right)
    return (left_parts > right_parts) - (left_parts < right_parts)
