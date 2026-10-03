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

    def start(self, mode="offline", results=("[55,2]", "[545,2]"), delay=0, timeout=500,
              exit_code=0, maximum=1, ttl=1000, returns=20):
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
                           PONG_TIMEOUT=str(timeout), PONG_TTL=str(ttl), PONG_MAX_DECISIONS=str(maximum), PONG_MAX_RETURNS=str(returns))
        flags = [f"--allow-net=127.0.0.1:{self.port}", "--allow-read=web", "--allow-clock=monotonic"]
        flags += [f"--allow-env={name}" for name in ("PONG_MODE", "PONG_PORT", "PONG_INTERVAL",
                                                     "PONG_TIMEOUT", "PONG_TTL", "PONG_MAX_DECISIONS", "PONG_MAX_RETURNS")]
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

    def edit_state(self, updates):
        path = self.cwd / "build" / "state.json"
        state = json.loads(path.read_text())
        for index, value in updates.items():
            state[index] = value
        path.write_text(json.dumps(state))

    def test_successful_independent_mock_targets_and_request_cap(self):
        self.start("jev")
        self.control(True)
        state = self.until(lambda s: bool(s["players"][1]["history"]))
        self.assertEqual([p["target_y"] for p in state["players"]], [300, 545])
        self.assertEqual([p["requests"] for p in state["players"]], [0, 1])
        right = json.loads((self.cwd / "build" / "right-job.json").read_text())
        self.assertEqual(right["player"], "right")
        self.assertEqual(right["predicted_impact_y"], 590)
        self.assertEqual(right["ticks_until_own_paddle"], 58)
        # Switch to a new incoming flight for left; no extra request on outgoing side.
        self.edit_state({2: -8, 82: 1})
        state = self.until(lambda s: bool(s["players"][0]["history"]))
        self.assertEqual([p["target_y"] for p in state["players"]], [55, 545])
        left = json.loads((self.cwd / "build" / "left-job.json").read_text())
        self.assertEqual(left["player"], "left")
        self.assertEqual(left["history"], [])
        self.edit_state({2: 8, 82: 2})
        stopped = self.get()
        self.assertFalse(stopped["running"])
        self.assertEqual(stopped["stop_reason"], 2)
        self.assertEqual([p["requests"] for p in stopped["players"]], [1, 1])
        self.control(False)
        with self.assertRaises(urllib.error.HTTPError) as error:
            self.control(True)
        self.assertEqual(error.exception.code, 409)

    def test_malformed_or_illegal_mock_targets_hold_position(self):
        for result in ("broken", "[99,2]"):
            with self.subTest(result=result):
                if self.process:
                    self.process.terminate(); self.process.wait(); self.process.stderr.close()
                self.start("jev", results=(result, result))
                self.control(True)
                state = self.until(lambda s: bool(s["players"][1]["history"]))
                self.assertEqual(state["players"][1]["status"], 4)
                self.assertEqual(state["players"][1]["target_y"], 300)
                self.assertEqual(state["players"][1]["action"], 0)

    def test_failed_worker_cannot_apply_a_target(self):
        self.start("jev", exit_code=1)
        self.control(True)
        state = self.until(lambda s: bool(s["players"][1]["history"]))
        self.assertEqual(state["players"][1]["status"], 4)
        self.assertEqual(state["players"][1]["target_y"], 300)

    def test_delayed_worker_times_out_while_physics_continues(self):
        self.start("jev", delay=1, timeout=120)
        self.control(True)
        state = self.until(lambda s: bool(s["players"][1]["history"]))
        self.assertGreater(state["state"][8], 0)
        self.assertEqual(state["players"][1]["status"], 3)
        self.assertEqual(state["players"][1]["target_y"], 300)
        time.sleep(0.15)
        self.assertEqual((self.cwd / "build" / "right-result.json").read_text(), "[0,4]")

    def test_return_limit_autopauses_and_cannot_be_resumed(self):
        self.start(returns=1)
        self.edit_state({0: 960, 1: 300, 2: 8, 3: 0})
        self.control(True)
        stopped = self.until(lambda s: not s["running"])
        self.assertEqual(stopped["returns"], 1)
        self.assertEqual(stopped["rally"], 1)
        self.assertEqual(stopped["longest_rally"], 1)
        self.assertEqual(stopped["stop_reason"], 1)
        self.control(False)
        with self.assertRaises(urllib.error.HTTPError):
            self.control(True)

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
        self.assertEqual([p["thinking"] for p in state["players"]], [False, True])
        paused = self.control(False)
        self.assertFalse(any(p["thinking"] for p in paused["players"]))
        self.assertEqual([p["action"] for p in paused["players"]], [0, 0])


if __name__ == "__main__":
    unittest.main()
