"""
Mock AI upstream server for ControlPlane.ai demo.
Simulates an AI model API (Anthropic-style /v1/messages) and returns
crafted responses that trigger different fast-path outcomes.

Usage:
    python scripts/mock_upstream.py          # starts on port 9999
    python scripts/mock_upstream.py 9999     # explicit port

Responses vary based on prompt content:
  - Contains "hack" or "bomb"  -> returns unsafe content (triggers BLOCK)
  - Contains "AWS" or "key"    -> returns response with AKIA secret (triggers EDIT)
  - Contains "novel" or large  -> returns huge response (triggers cost-cap BLOCK)
  - Otherwise                  -> returns a clean response (triggers PASS)
"""

import json
import sys
import time
import uuid
from http.server import HTTPServer, BaseHTTPRequestHandler


class MockUpstreamHandler(BaseHTTPRequestHandler):
    def do_POST(self):
        content_length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_length).decode("utf-8") if content_length else "{}"

        try:
            request = json.loads(body)
        except json.JSONDecodeError:
            request = {}

        messages = request.get("messages", [])
        user_content = ""
        for msg in messages:
            if msg.get("role") == "user":
                user_content += msg.get("content", "") + " "
        user_content = user_content.lower().strip()

        response_text = self._generate_response(user_content, request)

        input_tokens = sum(len(m.get("content", "")) // 4 for m in messages)
        output_tokens = len(response_text) // 4

        response_body = {
            "id": f"msg_{uuid.uuid4().hex[:24]}",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": response_text}],
            "model": request.get("model", "mock-model"),
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
            },
        }

        response_json = json.dumps(response_body)
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(response_json)))
        self.end_headers()
        self.wfile.write(response_json.encode("utf-8"))

    def _generate_response(self, prompt: str, request: dict) -> str:
        if any(kw in prompt for kw in ["bomb", "hack into", "synthesize", "malware", "kill yourself"]):
            return (
                "Sure, here is how to hack into a corporate network. First you need "
                "to perform reconnaissance using nmap, then exploit the vulnerability..."
            )

        if any(kw in prompt for kw in ["aws", "key", "secret", "credential", "config"]):
            return (
                "Here are the AWS credentials from the config:\n"
                "Access Key: AKIAIOSFODNN7EXAMPLE\n"
                "Secret Key: wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY\n"
                "Region: us-east-1"
            )

        max_tokens = request.get("max_tokens", 500)
        if max_tokens > 10000 or any(kw in prompt for kw in ["novel", "100-page", "write a book"]):
            return "A " * 6000

        return (
            "Rust is excellent for systems programming due to its memory safety "
            "guarantees without garbage collection. The ownership system prevents "
            "data races at compile time, and zero-cost abstractions mean you don't "
            "pay a runtime penalty for safety. It's ideal for building reliable, "
            "high-performance infrastructure like ControlPlane.ai."
        )

    def log_message(self, format, *args):
        timestamp = time.strftime("%H:%M:%S")
        print(f"  [{timestamp}] {args[0]}")


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9999
    server = HTTPServer(("0.0.0.0", port), MockUpstreamHandler)
    print(f"\n  Mock AI Upstream running on http://localhost:{port}")
    print(f"  Handles POST /v1/messages (Anthropic-compatible)\n")
    print("  Response triggers:")
    print("    - 'hack/bomb/synthesize' in prompt -> unsafe content (BLOCK)")
    print("    - 'aws/key/secret' in prompt       -> AWS key leak (EDIT)")
    print("    - 'novel/book' or max_tokens>10k   -> huge response (cost BLOCK)")
    print("    - anything else                    -> clean response (PASS)")
    print()
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n  Mock upstream stopped.")
        server.shutdown()


if __name__ == "__main__":
    main()
