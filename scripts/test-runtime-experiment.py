"""Verify runtime acceptance and transport boundaries without claiming OHOS execution."""

import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import contextlib
import io

spec = importlib.util.spec_from_file_location('runtime', Path(__file__).with_name('runtime-experiment.py'))
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)


class LocalTransport(runtime.Experiment):
    fail_receive = False
    fail_cleanup = False
    fail_remote = None

    def hdc(self, name, *args, timeout=None):
        if name == self.fail_remote:
            marker = args[1].split('PULSE-SHELL-')[1].split(':')[0]
            return '\nPULSE-SHELL-' + marker + ':1\n'
        if name in ('guest-identity', 'guest-paths', 'guest-loader', 'guest-dependencies'):
            # Platform checks require a real guest; test only the transport protocol here.
            marker = args[1].split('PULSE-SHELL-')[1].split(':')[0]
            return '\nPULSE-SHELL-' + marker + ':0\n'
        if name == 'cleanup' and self.fail_cleanup:
            raise runtime.InfrastructureError('simulated cleanup transport failure')
        if args[0] == 'shell':
            return self.command(name, ['sh', '-c', args[1]], timeout)
        if args[:2] == ('file', 'recv') and self.fail_receive:
            raise runtime.InfrastructureError('simulated receive failure')
        shutil.copy2(args[2], args[3])
        return ''

    def command(self, name, args, timeout=None):
        if name in ('hdc-version', 'targets'):
            return 'local test double'
        return super().command(name, args, timeout)


class RuntimeTests(unittest.TestCase):
    def run_case(self, body, **flags):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            build = root / 'build'
            output = root / 'output'
            build.mkdir()
            output.mkdir()
            binary = build / 'fixture'
            binary.write_text('#!/bin/sh\n' + body)
            binary.chmod(0o700)
            report = {'source_commit': 'test', 'source_dirty': True,
                      'stages': {'link': {'status': 'passed'}},
                      'artifact': {'path': 'fixture', 'sha256': hashlib.sha256(binary.read_bytes()).hexdigest()}}
            (build / 'experiment.json').write_text(json.dumps(report))
            config = {'guest_base': raw, 'connect_key': 'test', 'transport_timeout_seconds': 5, 'execution_timeout_seconds': 2}
            experiment = LocalTransport(output, build, config)
            for key, value in flags.items():
                setattr(experiment, key, value)
            with contextlib.redirect_stdout(io.StringIO()):
                experiment.run()
            saved = json.loads((output / 'runtime.json').read_text())
            return saved

    def test_pass(self):
        result = self.run_case('printf \'{"scenario_id":"file-round-trip","run_id":"%s","status":"passed"}\\n\' "$2"\n')
        self.assertEqual(result['runtime']['status'], 'passed')
        self.assertEqual(result['runtime']['guest_exit_code'], 0)
        self.assertEqual(result['cleanup']['status'], 'passed')

    def test_wrong_id(self):
        result = self.run_case('echo \'{"scenario_id":"file-round-trip","run_id":"stale","status":"passed"}\'\n')
        self.assertEqual(result['runtime']['status'], 'failed')

    def test_nonzero(self):
        self.assertEqual(self.run_case('echo assertion-failed >&2\nexit 37\n')['runtime']['guest_exit_code'], 37)
        self.assertEqual(self.run_case('exit 37\n')['runtime']['status'], 'failed')

    def test_receive_failure(self):
        self.assertEqual(self.run_case('exit 0\n', fail_receive=True)['runtime']['status'], 'infra_error')

    def test_cleanup_failure(self):
        body = 'printf \'{"scenario_id":"file-round-trip","run_id":"%s","status":"passed"}\\n\' "$2"\n'
        self.assertEqual(self.run_case(body, fail_cleanup=True)['runtime']['status'], 'infra_error')

    def test_duplicate_completion(self):
        line = json.dumps({'scenario_id': 'file-round-trip', 'run_id': 'r', 'status': 'passed'})
        self.assertFalse(runtime.completion_matches(line + '\n' + line, 'r'))
        self.assertFalse(runtime.completion_matches('', 'r'))

    def test_timeout_cannot_pass(self):
        result = self.run_case('sleep 20\n')
        self.assertEqual(result['runtime']['status'], 'timed_out')

    def test_missing_loader_is_identified(self):
        result = self.run_case('exit 0\n', fail_remote='guest-loader')
        self.assertEqual(result['runtime']['status'], 'infra_error')
        self.assertIn('guest-loader: guest exit code 1', result['runtime']['reason'])
        self.assertEqual(result['cleanup']['status'], 'not_run')

    def test_unresolved_dependencies_block_execution(self):
        result = self.run_case('exit 0\n', fail_remote='guest-dependencies')
        self.assertEqual(result['runtime']['status'], 'infra_error')
        self.assertIn('guest-dependencies', result['runtime']['reason'])
        self.assertEqual(result['cleanup']['status'], 'passed')
        self.assertFalse(any(command['name'] == 'execute' for command in result['commands']))


if __name__ == '__main__':
    unittest.main()
