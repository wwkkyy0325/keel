def rank(players):
    return sorted(players, key=lambda player: (-player["score"], player["name"]))
