import sys
import os

query = sys.argv[1].lower()
path = sys.argv[2] if len(sys.argv) > 2 else "stellar-lend/contracts/lending/src"

def search_file(fpath):
    try:
        with open(fpath, "r", encoding="utf-8", errors="replace") as f:
            for idx, line in enumerate(f, 1):
                if query in line.lower():
                    print(f"{fpath}:{idx}: {line.strip()}")
    except Exception as e:
        pass

if os.path.isfile(path):
    search_file(path)
else:
    for dirpath, _, filenames in os.walk(path):
        for fn in filenames:
            if fn.endswith(".rs") or fn.endswith(".md"):
                search_file(os.path.join(dirpath, fn))
