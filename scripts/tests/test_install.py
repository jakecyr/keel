"""Offline installer tests. No network requests or user installation paths."""
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

INSTALLER = Path(__file__).resolve().parents[1] / "install.sh"


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="keel-installer-tests-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.mockbin = self.root / "mockbin"
        self.mockbin.mkdir()
        self.fixtures = self.root / "fixtures"
        self.fixtures.mkdir()
        self.prefix = self.root / "install space"
        self.tmp = self.root / "tmp"
        self.tmp.mkdir()
        self.log = self.root / "downloads.jsonl"
        self.env = dict(os.environ, PATH=str(self.mockbin) + os.pathsep + os.environ["PATH"],
                        TMPDIR=str(self.tmp), INSTALL_TEST_FIXTURES=str(self.fixtures),
                        INSTALL_TEST_LOG=str(self.log), INSTALL_TEST_OS="Linux",
                        INSTALL_TEST_ARCH="x86_64")
        self.script("uname", '#!/bin/sh\ncase "$1" in -s) printf "%s\\n" "$INSTALL_TEST_OS";; -m) printf "%s\\n" "$INSTALL_TEST_ARCH";; *) exit 1;; esac\n')
        self.script("curl", f"#!{sys.executable}\n" + r'''
import json, os, pathlib, shutil, sys
args = sys.argv[1:]
with open(os.environ["INSTALL_TEST_LOG"], "a") as log:
    log.write(json.dumps(args) + "\n")
if os.environ.get("INSTALL_TEST_FAIL"):
    sys.exit(22)
assert args[args.index("--proto") + 1] == "=https"
assert args[args.index("--proto-redir") + 1] == "=https"
assert args[-1].startswith("https://github.com/")
source = pathlib.Path(os.environ["INSTALL_TEST_FIXTURES"]) / args[-1].rsplit("/", 1)[-1]
if not source.exists():
    sys.exit(22)
shutil.copyfile(source, args[args.index("--output") + 1])
''')
        self.asset = self.fixture()

    def script(self, name, text):
        path = self.mockbin / name
        path.write_text(text)
        path.chmod(0o755)

    def fixture(self, system="Linux", architecture="X64", member="keel", payload=None):
        asset = f"keel-{system}-{architecture}.tar.gz"
        payload = payload if payload is not None else b"#!/bin/sh\nprintf 'keel fixture\\n'\n"
        with tarfile.open(self.fixtures / asset, "w:gz") as archive:
            info = tarfile.TarInfo(member)
            info.size = len(payload)
            info.mode = 0o755
            archive.addfile(info, io.BytesIO(payload))
        digest = hashlib.sha256((self.fixtures / asset).read_bytes()).hexdigest()
        (self.fixtures / f"{asset}.sha256").write_text(f"{digest}  {asset}\n")
        return asset

    def run_install(self, *args, repository=True):
        command = ["sh", str(INSTALLER)]
        if repository:
            command += ["--repo", "example-owner/keel"]
        result = subprocess.run(command + list(args), env=self.env, text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
        self.assertEqual(list(self.tmp.iterdir()), [], "temporary files were not cleaned")
        return result

    def assert_failed(self, result, message):
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(message, result.stderr)

    def existing(self):
        directory = self.prefix / "bin"
        directory.mkdir(parents=True)
        target = directory / "keel"
        target.write_bytes(b"old compiler")
        target.chmod(0o755)
        return target

    def test_help_is_offline(self):
        result = self.run_install("--help", repository=False)
        self.assertEqual(result.returncode, 0)
        self.assertIn("published GitHub release", result.stdout)
        self.assertFalse(self.log.exists())

    def test_unknown_options_and_missing_values(self):
        for args, message in [
            (["--typo"], "unknown argument"),
            (["--repo"], "requires a value"), (["--prefix"], "requires a value"),
            (["--version"], "requires a value"),
        ]:
            with self.subTest(args=args):
                self.assert_failed(self.run_install(*args, repository=False), message)
        self.assertFalse(self.log.exists())

    def test_default_repository_uses_supplied_project_origin(self):
        result = self.run_install("--prefix", str(self.prefix), repository=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("https://github.com/jakecyr/keel/releases/latest/download/", self.log.read_text())

    def test_invalid_repository_version_and_paths(self):
        for repository in ["../keel", "owner/../keel", "https://example.com/keel",
                           "owner/repo;id", "owner/repo\nother/repo"]:
            self.assert_failed(self.run_install("--repo", repository, repository=False), "--repo")
        for version in ["../../main", "$(id)", "v1.2.3\nv9.0.0", "--help"]:
            self.assert_failed(self.run_install("--version", version), "--version")
        for path in ["relative", "/tmp/../bin", "/tmp/bin\nother"]:
            self.assert_failed(self.run_install("--bin-dir", path), "installation directory")
        self.assert_failed(self.run_install("--prefix", str(self.prefix), "--bin-dir", str(self.prefix)), "choose one")
        self.assertFalse(self.log.exists())

    def test_unsupported_platform_and_architecture_are_offline(self):
        self.env["INSTALL_TEST_OS"] = "Windows"
        self.assert_failed(self.run_install("--prefix", str(self.prefix)), "unsupported operating system")
        self.env["INSTALL_TEST_OS"] = "Linux"
        self.env["INSTALL_TEST_ARCH"] = "riscv64"
        self.assert_failed(self.run_install("--prefix", str(self.prefix)), "unsupported architecture")
        self.assertFalse(self.log.exists())

    def test_success_atomic_replace_permissions_path_and_https(self):
        target = self.existing()
        result = self.run_install("--prefix", str(self.prefix))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(str(target), result.stdout)
        self.assertIn("PATH", result.stdout)
        self.assertEqual(target.stat().st_mode & 0o777, 0o755)
        executed = subprocess.run([str(target)], capture_output=True, text=True, timeout=5)
        self.assertEqual(executed.stdout, "keel fixture\n")
        self.assertEqual(sorted(p.name for p in target.parent.iterdir()), ["keel"])
        requests = [json.loads(line) for line in self.log.read_text().splitlines()]
        self.assertEqual(len(requests), 2)
        self.assertTrue(requests[0][-1].endswith("/releases/latest/download/" + self.asset))
        self.assertTrue(requests[1][-1].endswith(self.asset + ".sha256"))
        self.assertFalse((self.root / ".profile").exists())

    def test_tagged_macos_arm64_asset_and_bin_dir(self):
        asset = self.fixture("macOS", "ARM64")
        self.env.update(INSTALL_TEST_OS="Darwin", INSTALL_TEST_ARCH="arm64")
        result = self.run_install("--version", "v0.1.0", "--bin-dir", str(self.prefix))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.prefix / "keel").is_file())
        self.assertIn(f"/releases/download/v0.1.0/{asset}", self.log.read_text())

    def test_linux_arm64_and_macos_x64_names_match_ci_convention(self):
        for system, machine, asset_os, asset_arch in [
            ("Linux", "aarch64", "Linux", "ARM64"),
            ("Darwin", "x86_64", "macOS", "X64"),
        ]:
            with self.subTest(system=system, machine=machine):
                asset = self.fixture(asset_os, asset_arch)
                self.env.update(INSTALL_TEST_OS=system, INSTALL_TEST_ARCH=machine)
                result = self.run_install("--prefix", str(self.prefix))
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(asset, self.log.read_text())

    def test_pipe_style_execution(self):
        result = subprocess.run(["sh", "-s", "--", "--prefix", str(self.prefix)],
                                input=INSTALLER.read_text(), env=self.env, text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.prefix / "bin" / "keel").is_file())
        self.assertEqual(list(self.tmp.iterdir()), [])

    def test_bash_pipe_and_copyable_path_for_quoted_directory(self):
        destination = self.root / "it's a directory"
        result = subprocess.run(["bash", "-s", "--", "--bin-dir", str(destination)],
                                input=INSTALLER.read_text(), env=self.env, text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        export = next(line.removeprefix("For this terminal: ") for line in result.stdout.splitlines()
                      if line.startswith("For this terminal: "))
        probe = subprocess.run(["sh", "-c", export + '\ncommand -v keel'], env=self.env,
                               text=True, capture_output=True, timeout=5)
        self.assertEqual(probe.returncode, 0, probe.stderr)
        self.assertEqual(probe.stdout.strip(), str(destination / "keel"))

    def test_download_failure_preserves_existing_binary(self):
        target = self.existing()
        self.env["INSTALL_TEST_FAIL"] = "1"
        self.assert_failed(self.run_install("--prefix", str(self.prefix)), "download failed")
        self.assertEqual(target.read_bytes(), b"old compiler")

    def test_checksum_mismatch_preserves_existing_binary(self):
        target = self.existing()
        (self.fixtures / f"{self.asset}.sha256").write_text(f"{'0' * 64}  {self.asset}\n")
        self.assert_failed(self.run_install("--prefix", str(self.prefix)), "checksum mismatch")
        self.assertEqual(target.read_bytes(), b"old compiler")

    def test_checksum_cannot_name_other_files(self):
        target = self.existing()
        (self.fixtures / f"{self.asset}.sha256").write_text(f"{'0' * 64}  ../other\n")
        self.assert_failed(self.run_install("--prefix", str(self.prefix)), "exactly the requested asset")
        self.assertEqual(target.read_bytes(), b"old compiler")

    def test_archive_paths_are_not_extracted(self):
        target = self.existing()
        self.fixture(member="../outside")
        self.assert_failed(self.run_install("--prefix", str(self.prefix)), "exactly one file")
        self.assertEqual(target.read_bytes(), b"old compiler")
        self.assertFalse((self.tmp / "outside").exists())

    def test_empty_payload_is_rejected(self):
        target = self.existing()
        self.fixture(payload=b"")
        self.assert_failed(self.run_install("--prefix", str(self.prefix)), "empty compiler")
        self.assertEqual(target.read_bytes(), b"old compiler")

    def test_archive_symlink_is_never_followed(self):
        target = self.existing()
        outside = self.root / "outside"
        outside.write_bytes(b"do not read or install")
        with tarfile.open(self.fixtures / self.asset, "w:gz") as archive:
            info = tarfile.TarInfo("keel")
            info.type = tarfile.SYMTYPE
            info.linkname = str(outside)
            archive.addfile(info)
        digest = hashlib.sha256((self.fixtures / self.asset).read_bytes()).hexdigest()
        (self.fixtures / f"{self.asset}.sha256").write_text(f"{digest}  {self.asset}\n")
        self.assert_failed(self.run_install("--prefix", str(self.prefix)), "empty compiler")
        self.assertEqual(target.read_bytes(), b"old compiler")
        self.assertEqual(outside.read_bytes(), b"do not read or install")

    def test_existing_symlink_and_directory_targets_are_refused(self):
        destination = self.root / "original"
        destination.write_bytes(b"do not touch")
        self.prefix.mkdir()
        (self.prefix / "keel").symlink_to(destination)
        self.assert_failed(self.run_install("--bin-dir", str(self.prefix)), "symlink")
        self.assertEqual(destination.read_bytes(), b"do not touch")
        (self.prefix / "keel").unlink()
        (self.prefix / "keel").mkdir()
        self.assert_failed(self.run_install("--bin-dir", str(self.prefix)), "not a regular file")
        self.assertFalse(self.log.exists())


if __name__ == "__main__":
    unittest.main()
