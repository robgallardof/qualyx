"""Tiny OpenAPI service with a seeded bug for contract testing.

GET /items/{item_id} returns an item for any non-negative id, except ids above 1000, which
crash with HTTP 500 (the seeded bug). Usage: python server.py <port>
"""

import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

SPEC = {
    "openapi": "3.0.3",
    "info": {"title": "Qualyx fixture API", "version": "1.0.0"},
    "paths": {
        "/items/{item_id}": {
            "get": {
                "operationId": "getItem",
                "parameters": [
                    {"name": "item_id", "in": "path", "required": True, "schema": {"type": "integer", "minimum": 0}}
                ],
                "responses": {
                    "200": {
                        "description": "An item",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "required": ["id", "name"],
                                    "properties": {"id": {"type": "integer"}, "name": {"type": "string"}},
                                }
                            }
                        },
                    }
                },
            }
        }
    },
}


class Handler(BaseHTTPRequestHandler):
    def _send(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path == "/openapi.json":
            return self._send(200, SPEC)
        if self.path.startswith("/items/"):
            raw = self.path[len("/items/"):]
            try:
                item_id = int(raw)
            except ValueError:
                return self._send(400, {"error": "item_id must be an integer"})
            if item_id < 0:
                return self._send(400, {"error": "item_id must be >= 0"})
            if item_id > 1000:
                return self._send(500, {"error": "boom"})  # seeded bug
            return self._send(200, {"id": item_id, "name": f"item {item_id}"})
        return self._send(404, {"error": "not found"})

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8000
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
