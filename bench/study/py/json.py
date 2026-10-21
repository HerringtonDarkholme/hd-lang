"""Encode/decode 400,000 records with CPython's standard JSON library."""
# This reference shares its filename with the stdlib module. Remove the
# script directory from the search path so `import json` finds the stdlib.
import sys
sys.path.pop(0)
import json

N = 6_000


def main():
    records = [{"id": i, "name": f"user{i}", "active": i % 3 == 0,
                "score": i * 37 % 1000, "tags": [f"t{i % 10}", f"g{i % 7}"]}
               for i in range(N)]
    text = json.dumps(records, separators=(",", ":"), ensure_ascii=False)
    back = json.loads(text)
    total = 0
    for record in back:
        total += record["score"] + len(record["tags"][1]) + int(record["active"])
    print(len(text), total)


if __name__ == "__main__":
    main()
