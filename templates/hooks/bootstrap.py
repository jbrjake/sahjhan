# _sahjhan_bootstrap.py — DO NOT MODIFY
# This hook protects Sahjhan's enforcement infrastructure.
# It is intentionally minimal and self-referential.
import os, sys, json

PROTECTED = ["enforcement/", "bin/sahjhan", "_sahjhan_bootstrap.py"]

event = json.loads(sys.stdin.read())
tool_name = event.get("tool_name", "")
if tool_name not in ("Write", "Edit"):
    print(json.dumps({"decision": "allow"}))
    sys.exit(0)

path = event.get("tool_input", {}).get("file_path", "")
cwd = event.get("cwd", os.getcwd())
resolved = os.path.realpath(os.path.join(cwd, path)) if path else ""

for p in PROTECTED:
    full = os.path.realpath(os.path.join(cwd, p))
    if resolved.startswith(full) or resolved == full:
        print(json.dumps({"decision": "block",
            "reason": f"BLOCKED: {path} is protected enforcement infrastructure."}))
        sys.exit(0)

print(json.dumps({"decision": "allow"}))
