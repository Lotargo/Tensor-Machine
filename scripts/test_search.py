import urllib.request
import json

data = json.dumps({
    "tool": "search_text",
    "args": {"query": "как звали мать Арсения?", "final_top_k": 5},
    "session_id": "test-1",
    "sequence_number": 1
}).encode("utf-8")

req = urllib.request.Request(
    "http://127.0.0.1:8080/mcp",
    data=data,
    headers={"Content-Type": "application/json"},
    method="POST"
)

try:
    with urllib.request.urlopen(req, timeout=30) as resp:
        result = json.loads(resp.read().decode("utf-8"))
        body = result.get("body", b"")
        if isinstance(body, list):
            body = bytes(body).decode("utf-8", errors="replace")
        print(body)
except Exception as e:
    print(f"Error: {e}")
