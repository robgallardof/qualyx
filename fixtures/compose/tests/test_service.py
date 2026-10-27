"""Integration test against a real container started by Qualyx from compose.yaml."""

import os
import urllib.request


def test_web_service_answers():
    addr = os.environ["QUALYX_SVC_WEB_80"]  # host:port published by compose
    with urllib.request.urlopen(f"http://{addr}/", timeout=10) as r:
        assert r.status == 200
        assert b"nginx" in r.read().lower()
