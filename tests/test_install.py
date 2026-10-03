import hashlib
import importlib.util
import os
import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]

class InstallerTests(unittest.TestCase):
    def run_installer(self, corrupt=False, architecture="x86_64", system="Linux", existing=None, home_name="home"):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        base = pathlib.Path(temporary.name)
        home = base / home_name
        home.mkdir()
        mocks = base / "mocks"
        mocks.mkdir()
        binary = b"#!/bin/sh\nprintf 'refreshagent 0.1.2\\n'\n"
        payload = base / "payload"
        payload.write_bytes(binary)
        checksum = base / "checksum"
        checksum.write_text(("0" * 64 if corrupt else hashlib.sha256(binary).hexdigest()) + "  refreshagent\n")
        curl = mocks / "curl"
        curl.write_text("""#!/usr/bin/env python3
import sys,os,shutil
args=sys.argv[1:]
if '--write-out' in args:
 print('https://github.com/RefreshAgent/refreshagent/releases/tag/v0.1.2',end='')
else:
 destination=args[args.index('--output')+1]
 shutil.copyfile(os.environ['MOCK_CHECKSUM'] if args[-1].endswith('.sha256') else os.environ['MOCK_PAYLOAD'],destination)
""")
        curl.chmod(0o755)
        uname = mocks / "uname"
        uname.write_text(f"#!/bin/sh\ncase \"$1\" in -s) echo '{system}';; -m) echo '{architecture}';; esac\n")
        uname.chmod(0o755)
        if existing:
            (home / ".local/bin").mkdir(parents=True)
            (home / ".local/bin/refreshagent").write_bytes(existing)
        env = {**os.environ, "HOME": str(home), "SHELL": "/bin/zsh", "PATH": str(mocks) + ":" + os.environ["PATH"], "MOCK_PAYLOAD": str(payload), "MOCK_CHECKSUM": str(checksum)}
        result = subprocess.run(["sh", str(ROOT / "install.sh")], env=env, capture_output=True, text=True)
        return result, home, env

    def test_install_and_path_setup_without_compiler(self):
        result, home, env = self.run_installer(home_name='home with spaces')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((home / '.local/bin/refreshagent').exists())
        self.assertIn('export PATH=', (home / '.zprofile').read_text())
        self.assertIn('Open a new terminal', result.stdout)
        second = subprocess.run(['sh', str(ROOT / 'install.sh')], env=env, capture_output=True, text=True)
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual((home / '.zprofile').read_text().count('# RefreshAgent user installation'), 1)

    def test_corruption_preserves_existing_install(self):
        old = b'previous executable'
        result, home, _ = self.run_installer(corrupt=True, existing=old)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((home / '.local/bin/refreshagent').read_bytes(), old)
        self.assertIn('Checksum mismatch', result.stderr)

    def test_unsupported_platform_fails_before_install(self):
        result, home, _ = self.run_installer(system='Windows')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((home / '.local/bin/refreshagent').exists())

    def test_mac_arm_selection(self):
        result, _, _ = self.run_installer(system='Darwin', architecture='arm64')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('aarch64-apple-darwin', result.stdout)

    def test_shell_special_characters_are_not_executed(self):
        result, home, _ = self.run_installer(home_name='home$NOT_A_VARIABLE`false`')
        self.assertEqual(result.returncode, 0, result.stderr)
        line = (home / '.zprofile').read_text()
        self.assertIn('\\$NOT_A_VARIABLE', line)
        loaded = subprocess.run(['sh', '-c', '. "$1"; command -v refreshagent', 'sh', str(home / '.zprofile')], capture_output=True, text=True)
        self.assertEqual(loaded.stdout.strip(), str(home / '.local/bin/refreshagent'))

    def test_formula_checksums_and_architectures(self):
        spec = importlib.util.spec_from_file_location('homebrew', ROOT / 'scripts/update_homebrew.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        formula = module.formula('v0.1.2', {t: 'a' * 64 for t in module.TARGETS})
        self.assertEqual(formula.count('sha256 '), 4)
        self.assertNotIn('cargo', formula)
        with self.assertRaises(ValueError):
            module.formula('v0.1.2', {t: 'bad' for t in module.TARGETS})

if __name__ == '__main__':
    unittest.main()
