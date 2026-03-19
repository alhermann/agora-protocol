#!/usr/bin/env python3
"""Simple HTTP server that serves the dashboard dist/ and proxies /api to the daemon."""
import http.server
import urllib.request
import urllib.error
import sys
import os

API_TARGET = os.environ.get("AGORA_API_URL", "http://127.0.0.1:7313")
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8080
DIST_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "dashboard", "dist")

class ProxyHandler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=DIST_DIR, **kwargs)

    def do_GET(self):
        if self.path.startswith("/api/"):
            self._proxy("GET")
        else:
            # Serve static files; fallback to index.html for SPA routes
            path = self.path.split("?")[0]
            full = os.path.join(DIST_DIR, path.lstrip("/"))
            if not os.path.exists(full) or os.path.isdir(full):
                self.path = "/index.html"
            super().do_GET()

    def do_POST(self):
        self._proxy("POST")

    def do_PUT(self):
        self._proxy("PUT")

    def do_PATCH(self):
        self._proxy("PATCH")

    def do_DELETE(self):
        self._proxy("DELETE")

    def _proxy(self, method):
        api_path = self.path.replace("/api/", "/", 1)
        url = f"{API_TARGET}{api_path}"

        headers = {}
        for key in ["Content-Type", "Accept"]:
            if key in self.headers:
                headers[key] = self.headers[key]

        body = None
        content_length = self.headers.get("Content-Length")
        if content_length:
            body = self.rfile.read(int(content_length))

        try:
            req = urllib.request.Request(url, data=body, headers=headers, method=method)
            with urllib.request.urlopen(req, timeout=30) as resp:
                data = resp.read()
                self.send_response(resp.status)
                self.send_header("Content-Type", resp.headers.get("Content-Type", "application/json"))
                self.send_header("Content-Length", len(data))
                self.send_header("Access-Control-Allow-Origin", "*")
                self.end_headers()
                self.wfile.write(data)
        except urllib.error.HTTPError as e:
            data = e.read()
            self.send_response(e.code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", len(data))
            self.end_headers()
            self.wfile.write(data)
        except Exception as e:
            msg = str(e).encode()
            self.send_response(502)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", len(msg))
            self.end_headers()
            self.wfile.write(msg)

    def log_message(self, format, *args):
        pass  # Suppress request logs

    def end_headers(self):
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, PUT, PATCH, DELETE, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        super().end_headers()

    def do_OPTIONS(self):
        self.send_response(204)
        self.end_headers()

import socket

class DualStackServer(http.server.HTTPServer):
    address_family = socket.AF_INET6

    def server_bind(self):
        self.socket.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 0)
        super().server_bind()

if __name__ == "__main__":
    print(f"Serving dashboard from {DIST_DIR}")
    print(f"API proxy → {API_TARGET}")
    print(f"http://localhost:{PORT}")
    try:
        server = DualStackServer(("::", PORT), ProxyHandler)
    except OSError:
        server = http.server.HTTPServer(("0.0.0.0", PORT), ProxyHandler)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
