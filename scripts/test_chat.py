import urllib.request
import json
import sys

data = json.dumps({
    "messages": [
        {"role": "user", "content": "как звали мать Арсения?"}
    ],
    "use_tools": True,
    "selected_doc_uuid": None,
    "scope_to_selected_document": False
}).encode("utf-8")

req = urllib.request.Request(
    "http://127.0.0.1:8080/chat",
    data=data,
    headers={"Content-Type": "application/json"},
    method="POST"
)

try:
    with urllib.request.urlopen(req, timeout=60) as resp:
        raw = resp.read()
        text = raw.decode("utf-8", errors="replace")
        sys.stdout.buffer.write(text.encode("utf-8", errors="replace"))
        sys.stdout.buffer.write(b"\n")
except Exception as e:
    print(f"Error: {e}")
