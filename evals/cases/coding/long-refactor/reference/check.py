"""Fail while any service module still uses the old limit."""

import importlib


def main():
    failures = []
    for i in range(20):
        module = importlib.import_module(f"svc_{i:02d}")
        if module.LIMIT != 25 or module.apply(30) != 25:
            failures.append(f"svc_{i:02d}: LIMIT={module.LIMIT} apply(30)={module.apply(30)}")
    if failures:
        print("
".join(failures))
        raise SystemExit(1)
    print("all twenty modules allow 25")


if __name__ == "__main__":
    main()
