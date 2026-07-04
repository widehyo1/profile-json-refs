import json
import sys

n = int(sys.argv[1])
for i in range(n):
    row = {
        "id": f"id-{i}",
        "status": ["A", "B", "C"][i % 3],
        "payload": {} if i == 0 else {"x": i, "flag": i % 2 == 0},
        "items": [
            {"kind": "amount", "value": i},
            {"kind": "code", "value": str(i % 10)},
        ],
    }
    print(json.dumps(row, separators=(",", ":")))
