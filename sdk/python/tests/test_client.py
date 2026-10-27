"""Exercise the SDK over real loopback HTTP connections, without mocking urllib."""
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import json
import sys
import threading
import unittest
from urllib.error import HTTPError

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from qualyx_client import QualyxClient


@contextmanager
def server(respond):
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            respond(self)

        def do_POST(self):
            respond(self)

        def log_message(self, *_):
            pass

    httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{httpd.server_port}"
    finally:
        httpd.shutdown()
        httpd.server_close()
        thread.join()


def reply(request, body):
    payload = json.dumps(body).encode()
    request.send_response(200)
    request.send_header("Content-Type", "application/json")
    request.send_header("Content-Length", str(len(payload)))
    request.end_headers()
    request.wfile.write(payload)


class ClientTests(unittest.TestCase):
    def test_redirect_never_forwards_credentials_or_repeats_a_request(self):
        leaked = []

        def destination(request):
            leaked.append(request.headers.get("Authorization"))
            reply(request, {"jobs": []})

        with server(destination) as other:
            for code in (301, 302, 303, 307, 308):
                with self.subTest(code=code):
                    def redirect(request):
                        request.send_response(code)
                        request.send_header("Location", other + "/capture")
                        request.send_header("Content-Length", "0")
                        request.end_headers()

                    with server(redirect) as base:
                        client = QualyxClient(base, "synthetic-test-token")
                        with self.assertRaises(HTTPError) as error:
                            client.runs()
                        self.assertEqual(error.exception.code, code)
                        error.exception.close()
                        with self.assertRaises(HTTPError) as error:
                            client.start("/trusted/project")
                        error.exception.close()
        self.assertEqual(leaked, [])

    def test_normal_requests_keep_auth_and_encode_untrusted_path_segments(self):
        received = []

        def respond(request):
            received.append((request.path, request.headers.get("Authorization")))
            reply(request, {"state": "finished", "manifest": {"status": "failed"}})

        with server(respond) as base:
            result = QualyxClient(base, "synthetic-test-token").status("id/with?query")
        self.assertEqual(result["manifest"]["status"], "failed")
        self.assertEqual(received, [("/v1/runs/id%2Fwith%3Fquery", "Bearer synthetic-test-token")])


if __name__ == "__main__":
    unittest.main()
