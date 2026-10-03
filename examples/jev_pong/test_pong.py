"""Offline native HTTP/process integration tests; never read keys or call Jev."""
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import unittest
import urllib.error
import urllib.request

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent


class NativePongTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.compile_dir = tempfile.TemporaryDirectory(prefix="pong-compile-")
        cls.binary = Path(cls.compile_dir.name) / "pong"
        subprocess.run(["cargo", "run", "--locked", "--manifest-path", str(ROOT / "Cargo.toml"),
                        "--", "build", str(HERE), "-o", str(cls.binary)], cwd=ROOT, check=True,
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)

        cls.worker = Path(cls.compile_dir.name) / "left-worker"
        subprocess.run(["cargo", "run", "--locked", "--manifest-path", str(ROOT / "Cargo.toml"),
                        "--", "build", str(HERE / "left-worker.json"), "-o", str(cls.worker)],
                       cwd=ROOT, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    @classmethod
    def tearDownClass(cls):
        cls.compile_dir.cleanup()

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="pong-test-")
        self.cwd = Path(self.directory.name)
        (self.cwd / "build").mkdir()
        shutil.copytree(HERE / "web", self.cwd / "web")
        self.process = None

    def tearDown(self):
        if self.process:
            self.process.terminate()
            try:
                self.process.wait(timeout=4)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
            self.process.stderr.close()
        self.directory.cleanup()

    def start(self, mode="offline", results=("[-1,2]", "[1,2]"), delay=0, timeout=500,
              exit_code=0, maximum=1, ttl=1000):
        for side, result in zip(("left", "right"), results):
            path = self.cwd / "build" / f"{side}-worker"
            # This mock executable exists only in the test sandbox, not the app.
            path.write_text(f"#!{sys.executable}\nimport pathlib,time\ntime.sleep({delay!r})\n"
                            f"pathlib.Path('build/{side}-result.json').write_text({result!r})\n"
                            f"raise SystemExit({exit_code})\n")
            path.chmod(0o700)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            self.port = sock.getsockname()[1]
        environment = os.environ.copy()
        environment.pop("JEV_API_KEY", None)
        environment.update(PONG_MODE=mode, PONG_PORT=str(self.port), PONG_INTERVAL="100",
                           PONG_TIMEOUT=str(timeout), PONG_TTL=str(ttl), PONG_MAX_DECISIONS=str(maximum))
        flags = [f"--allow-net=127.0.0.1:{self.port}", "--allow-read=web", "--allow-clock=monotonic"]
        flags += [f"--allow-env={name}" for name in ("PONG_MODE", "PONG_PORT", "PONG_INTERVAL",
                                                     "PONG_TIMEOUT", "PONG_TTL", "PONG_MAX_DECISIONS")]
        flags += [f"--allow-{mode}=build/{file}" for mode, files in (
            ("read", ["state.json", "left-result.json", "right-result.json"]),
            ("write", ["state.json", "left-result.json", "right-result.json", "left-job.json", "right-job.json"])) for file in files]
        flags += [f"--allow-exec=./build/{side}-worker" for side in ("left", "right")]
        self.process = subprocess.Popen([str(self.binary), *flags], cwd=self.cwd, env=environment,
                                        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                self.fail("Native server exited: " + self.process.stderr.read().decode())
            try:
                return self.get()
            except (urllib.error.URLError, ConnectionError):
                time.sleep(0.02)
        self.fail("Native server did not start")

    def request(self, path, body=None, headers=None):
        request = urllib.request.Request(f"http://127.0.0.1:{self.port}{path}",
                                         data=None if body is None else json.dumps(body).encode(),
                                         headers=headers or {"Content-Type": "application/json"})
        return urllib.request.urlopen(request, timeout=2)

    def get(self):
        with self.request("/api/state") as response:
            return json.load(response)

    def control(self, running):
        with self.request("/api/control", {"running": running}) as response:
            return json.load(response)

    def until(self, predicate, seconds=2):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            value = self.get()
            if predicate(value):
                return value
            time.sleep(0.025)
        self.fail(f"Expected state not reached: {value}")

    def test_native_assets_offline_tick_pause_and_control_validation(self):
        initial = self.start()
        self.assertFalse(initial["running"])
        with self.request("/") as response:
            self.assertIn("text/html", response.headers["Content-Type"])
            self.assertIn(b"Decision /", response.read())
        with self.request("/app.js") as response:
            self.assertIn(b"requestAnimationFrame", response.read())
        self.control(True)
        moving = self.until(lambda s: s["state"][8] >= 5)
        self.assertNotEqual(moving["state"][:2], [500, 300])
        self.assertTrue(all(p["requests"] == 0 for p in moving["players"]))
        paused = self.control(False)
        time.sleep(0.08)
        self.assertEqual(self.get()["state"], paused["state"])
        with self.assertRaises(urllib.error.HTTPError) as error:
            self.control("yes")
        self.assertEqual(error.exception.code, 400)
        with self.assertRaises(urllib.error.HTTPError) as error:
            self.request("/api/control", {"running": True}, {"Origin": "https://evil.example"})
        self.assertEqual(error.exception.code, 403)

    def test_successful_independent_mock_decisions_and_request_cap(self):
        self.start("jev")
        self.control(True)
        state = self.until(lambda s: all(p["history"] for p in s["players"]))
        self.assertEqual([p["action"] for p in state["players"]], [-1, 1])
        self.assertEqual([p["status"] for p in state["players"]], [2, 2])
        observations = [json.loads((self.cwd / "build" / f"{side}-job.json").read_text()) for side in ("left", "right")]
        self.assertEqual([o["player"] for o in observations], ["left", "right"])
        self.assertEqual([o["history"] for o in observations], [[], []])
        self.assertTrue(all(p["latency_ms"] >= 0 for p in state["players"]))
        expired = self.until(lambda s: all(p["status"] == 5 for p in s["players"]))
        self.assertEqual([p["requests"] for p in expired["players"]], [1, 1])
        self.assertEqual([p["action"] for p in expired["players"]], [0, 0])

    def test_malformed_or_illegal_mock_choices_fall_back(self):
        self.start("jev", results=("broken", "[99,2]"))
        self.control(True)
        state = self.until(lambda s: all(p["history"] for p in s["players"]))
        self.assertEqual([p["status"] for p in state["players"]], [4, 4])
        self.assertEqual([p["action"] for p in state["players"]], [0, 0])

    def test_failed_worker_cannot_apply_a_result(self):
        self.start("jev", exit_code=1)
        self.control(True)
        state = self.until(lambda s: all(p["history"] for p in s["players"]))
        self.assertEqual([p["status"] for p in state["players"]], [4, 4])
        self.assertEqual([p["action"] for p in state["players"]], [0, 0])

    def test_delayed_workers_timeout_while_physics_continues(self):
        self.start("jev", delay=1, timeout=120)
        self.control(True)
        state = self.until(lambda s: all(p["history"] for p in s["players"]))
        self.assertGreater(state["state"][8], 0)
        self.assertEqual([p["status"] for p in state["players"]], [3, 3])
        self.assertEqual([p["action"] for p in state["players"]], [0, 0])
        time.sleep(0.15)
        self.assertEqual([(self.cwd / "build" / f"{side}-result.json").read_text() for side in ("left", "right")], ["[0,4]", "[0,4]"])

    def test_real_native_worker_without_credentials_returns_safe_error(self):
        sandbox = self.cwd / "worker-work"
        (sandbox / "build").mkdir(parents=True)
        (self.cwd / "jev").mkdir()
        (self.cwd / "jev" / ".env").write_text("JEV_API_KEY=\n")
        (sandbox / "build" / "left-job.json").write_text('{"player":"left"}')
        environment = os.environ.copy()
        environment.pop("JEV_API_KEY", None)
        # No network grant: a missing key must return before any HTTP attempt.
        subprocess.run([str(self.worker), "--allow-read=build/left-job.json",
                        "--allow-write=build/left-result.json", "--allow-read=../jev/.env",
                        "--allow-env=JEV_API_KEY"], cwd=sandbox, env=environment,
                       capture_output=True, check=True, timeout=2)
        self.assertEqual(json.loads((sandbox / "build" / "left-result.json").read_text()), [0, 4])

    def test_pause_cancels_pending_workers(self):
        self.start("jev", delay=1)
        state = self.control(True)
        self.assertTrue(all(p["thinking"] for p in state["players"]))
        paused = self.control(False)
        self.assertFalse(any(p["thinking"] for p in paused["players"]))
        self.assertEqual([p["action"] for p in paused["players"]], [0, 0])


if __name__ == "__main__":
    unittest.main()
