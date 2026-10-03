"""Offline integration tests. Python is test tooling, never the game server."""
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import tempfile
import time
import unittest
import urllib.error
import urllib.request

HERE = Path(__file__).resolve().parent
KEEL = HERE.parents[1] / 'target' / 'debug' / 'keel'


class NativeArenaTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not KEEL.is_file():
            raise RuntimeError('Run cargo build --locked before example tests')
        cls.temp = tempfile.TemporaryDirectory(prefix='keel-arena-test-')
        cls.root = Path(cls.temp.name)
        cls.example = cls.root / 'examples' / 'evolving_arena'
        cls.example.mkdir(parents=True)
        for file in HERE.iterdir():
            if file.suffix in ('.keel', '.json', '.txt'):
                shutil.copyfile(file, cls.example / file.name)
        shutil.copytree(HERE / 'public', cls.example / 'public')
        compiler = cls.root / 'target' / 'debug' / 'keel'
        compiler.parent.mkdir(parents=True)
        compiler.symlink_to(KEEL)
        cls.runtime = cls.example / 'runtime'
        cls.runtime.mkdir()
        (cls.runtime / 'mode.txt').write_text('demo')
        cls.compile('.', 'runtime/arena')
        cls.compile('worker-red.json', 'runtime/worker-0')
        cls.compile('worker-blue.json', 'runtime/worker-1')
        cls.support = ''.join((cls.example / n).read_text() for n in
                              ('common.keel', 'referee.keel', 'candidate_support.keel'))
        for player in range(2):
            source = f'runtime/ability-{player}-0.keel'
            (cls.example / source).write_text(cls.support + '\nfn ability(state: read List<Int>) -> List<Int> { return [3,3,2,2] }\n')
            cls.compile(source, f'runtime/ability-{player}-0')

    @classmethod
    def compile(cls, source, output):
        subprocess.run([str(KEEL), 'build', source, '-o', output], cwd=cls.example,
                       check=True, capture_output=True, timeout=40)

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def setUp(self):
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            self.port = sock.getsockname()[1]
        args = [str(self.runtime / 'arena'), f'--allow-net=127.0.0.1:{self.port}',
                '--allow-env=ARENA_PORT', '--allow-clock=monotonic', '--allow-read=public',
                '--allow-read=runtime/mode.txt', '--allow-read=runtime/state.json',
                '--allow-write=runtime/state.json', '--allow-write=runtime/observation.json']
        for player in range(2):
            args.append(f'--allow-exec=runtime/worker-{player}')
            for slot in range(2):
                args.append(f'--allow-exec=runtime/ability-{player}-{slot}')
            for kind in ('active', 'feedback', 'history', 'result', 'task'):
                args += [f'--allow-read=runtime/{kind}-{player}.json',
                         f'--allow-write=runtime/{kind}-{player}.json']
        self.logs = tempfile.TemporaryFile()
        self.server = subprocess.Popen(args, cwd=self.example,
                                       env={**os.environ, 'ARENA_PORT': str(self.port)},
                                       stdout=self.logs, stderr=self.logs, start_new_session=True)
        for _ in range(100):
            try:
                self.get('/')
                break
            except (OSError, urllib.error.URLError):
                if self.server.poll() is not None:
                    self.logs.seek(0)
                    self.fail(self.logs.read().decode())
                time.sleep(.03)
        else:
            self.fail('native HTTP server did not start')

    def tearDown(self):
        if self.server.poll() is None:
            os.killpg(self.server.pid, signal.SIGTERM)
        self.server.wait(timeout=5)
        self.logs.close()

    def get(self, path):
        with urllib.request.urlopen(f'http://127.0.0.1:{self.port}{path}', timeout=5) as response:
            return response.read()

    def frame(self):
        return json.loads(self.get('/state'))

    def advance_immediately(self):
        state = json.loads((self.runtime / 'state.json').read_text())
        state[18] = 0
        (self.runtime / 'state.json').write_text(json.dumps(state))
        return self.frame()

    def test_static_files_private_files_and_pause(self):
        self.assertIn(b'Rewrite your advantage', self.get('/'))
        for path in ('/.env', '/runtime/state.json', '/%2e%2e/agent-prompt.txt'):
            with self.assertRaises(urllib.error.HTTPError):
                self.get(path)
        request = urllib.request.Request(f'http://127.0.0.1:{self.port}/pause',
                                         data=b'{}', headers={'Content-Type': 'application/json'})
        with urllib.request.urlopen(request, timeout=5) as response:
            self.assertEqual(response.status, 200)
        first = self.frame()['game']
        time.sleep(.13)
        second = self.frame()['game']
        self.assertEqual(first[0], second[0])
        self.assertEqual(second[27], 1)

    def test_workers_validate_and_activate_only_at_boundary(self):
        # Finished workers must be collected even if no browser polled before
        # their deadline. Completion is checked before elapsed-time rejection.
        for _ in range(200):
            results = [json.loads((self.runtime / f'result-{p}.json').read_text()) for p in range(2)]
            if all(result.get('ok') == 1 for result in results):
                break
            time.sleep(.03)
        else:
            self.fail(str(results))
        time.sleep(.06)
        state = json.loads((self.runtime / 'state.json').read_text())
        state[23] = state[24] = 0
        (self.runtime / 'state.json').write_text(json.dumps(state))
        for _ in range(150):
            frame = self.frame()
            if all(frame['game'][25:27]):
                break
            time.sleep(.03)
        else:
            self.fail(str(frame['feedback']))
        self.assertEqual([p['name'] for p in frame['players']], ['Standard issue'] * 2)
        self.assertIn('samples passed', frame['feedback'][0])
        for _ in range(151):
            frame = self.advance_immediately()
            if frame['game'][15] == 2:
                break
        self.assertEqual(frame['game'][15], 2)
        self.assertEqual([p['name'] for p in frame['players']], ['Last stand', 'Longbow'])
        self.assertNotEqual(frame['players'][0]['memory'], frame['players'][1]['memory'])
        for player in range(2):
            task = json.loads((self.runtime / f'task-{player}.json').read_text())
            self.assertEqual(task['player'], player)
            self.assertEqual(task['current']['name'], frame['players'][player]['name'])
            self.assertEqual(task['previous_round'][-1], player)
        self.assertEqual(frame['mode'], 'demo')

    def test_referee_rejects_runtime_budget_violation(self):
        bad = self.runtime / 'bad.keel'
        bad.write_text(self.support + '\nfn ability(state: read List<Int>) -> List<Int> { return [5,5,5,5] }\n')
        self.compile('runtime/bad.keel', 'runtime/ability-0-0')
        try:
            frame = self.advance_immediately()
            self.assertEqual(frame['game'][28], 1)
            self.assertEqual(frame['game'][7:11], [3, 3, 2, 2])
            self.assertIn('Runtime ability rejected', frame['feedback'][0])
        finally:
            self.compile('runtime/ability-0-0.keel', 'runtime/ability-0-0')

    def test_unseen_state_loop_is_bounded_and_disabled(self):
        source = self.runtime / 'loop.keel'
        source.write_text(self.support + '\nfn ability(state: read List<Int>) -> List<Int> { if list.at(state,2)==29 { while true { } } return [3,3,2,2] }\n')
        self.compile('runtime/loop.keel', 'runtime/ability-0-0')
        try:
            state = json.loads((self.runtime / 'state.json').read_text())
            state[3] = 29
            state[18] = 0
            (self.runtime / 'state.json').write_text(json.dumps(state))
            start = time.monotonic()
            frame = self.frame()
            self.assertLess(time.monotonic() - start, 2)
            self.assertEqual(frame['game'][28], 1)
            self.assertEqual(frame['game'][7:11], [3, 3, 2, 2])
            self.assertIn('Runtime ability rejected', frame['feedback'][0])
        finally:
            self.compile('runtime/ability-0-0.keel', 'runtime/ability-0-0')


class CandidateBoundaryTests(unittest.TestCase):
    def test_body_edit_cannot_append_declarations(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'candidate.keel'
            baseline = 'fn ability(state: read List<Int>) -> List<Int> { return [3,3,2,2] }\n'
            path.write_text(baseline)
            inspected = subprocess.run([str(KEEL), 'inspect', str(path), '--json'],
                                       capture_output=True, text=True, check=True)
            revision = json.loads(inspected.stdout)['revision']
            request = Path(directory) / 'edit.json'
            request.write_text(json.dumps({'base_revision': revision, 'target': 'fn:ability',
                                           'operation': 'replace_body', 'run': 'check',
                                           'source': '{ return [3,3,2,2] } fn injected() -> Int { return 1 }'}))
            changed = subprocess.run([str(KEEL), 'edit', str(path), '--request', str(request), '--json'],
                                     capture_output=True, text=True)
            self.assertNotEqual(changed.returncode, 0)
            self.assertEqual(path.read_text(), baseline)


if __name__ == '__main__':
    unittest.main()
