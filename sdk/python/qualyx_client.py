"""Qualyx HTTP v1 client, SDK version 0.1.0. Python standard library only."""
import json
from urllib.parse import urlencode, quote, urlsplit
from urllib.request import Request, urlopen

__version__ = "0.1.0"


class QualyxClient:
    """Authenticated client. Errors retain HTTP status via urllib.error.HTTPError."""

    def __init__(self, base_url, token, timeout=15):
        parsed = urlsplit(base_url)
        if parsed.scheme not in {"http", "https"} or not parsed.hostname or parsed.username or parsed.password or parsed.query or parsed.fragment:
            raise ValueError("expected an HTTP(S) API base URL without credentials, query or fragment")
        self.base_url, self.token, self.timeout = base_url.rstrip("/"), token, timeout

    def _request(self, route, body=None, text=False):
        req = Request(self.base_url + route,
                      data=None if body is None else json.dumps(body).encode(),
                      headers={"Authorization": "Bearer " + self.token, "Content-Type": "application/json"})
        with urlopen(req, timeout=self.timeout) as response:
            payload = response.read().decode()
        return payload if text else json.loads(payload)

    def start(self, project, *, timeout_secs=600, jobs=1, changed_since=None):
        """Enqueue a run; acceptance is not a passing test result."""
        return self._request("/v1/runs", {"project": str(project), "timeout_secs": timeout_secs, "jobs": jobs, "changed_since": changed_since})

    def status(self, run_id):
        """Read durable team status or a local API's active run."""
        return self._request("/v1/runs/" + quote(run_id, safe=""))

    def cancel(self, run_id):
        """Request cooperative cancellation."""
        return self._request("/v1/runs/" + quote(run_id, safe="") + "/cancel", {})

    def runs(self, project=None, limit=20):
        """List history using the server's configured local/team response."""
        query = {"limit": limit}
        if project is not None:
            query["project"] = str(project)
        return self._request("/v1/runs?" + urlencode(query))

    def artifact(self, run_id, project, name):
        """Read a known text artifact; credentials never enter URLs."""
        return self._request("/v1/runs/" + quote(run_id, safe="") + "/artifacts/" + quote(name, safe="") + "?" + urlencode({"project": str(project)}), text=True)
