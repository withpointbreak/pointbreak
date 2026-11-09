#!/usr/bin/env python3
"""Run the finite macOS append-progress gate from reviewed build/fixture receipts."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import xml.etree.ElementTree as ET


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def main():
    config_path = Path(sys.argv[1]).resolve(strict=True)
    config = json.loads(config_path.read_text())
    require(sys.platform == 'darwin' and config['host'] == 'macos', 'macOS gate only')
    require(config['schema'] == 'append-progress-gate-v1', 'gate schema')
    source = Path(config['source'])
    evidence = Path(config['evidence'])
    inventory = json.loads((source / 'scripts/append-progress-cases.json').read_text())
    require(len(inventory) == 55 and len({c['name'] for c in inventory}) == 55, 'case inventory')
    expected = [c for c in inventory if c['macos'] or c['pure'] or c['child_entry']]
    selected = sorted((c for c in expected if not c['child_entry']), key=lambda c: not c['pure'])
    require(len(selected) == 53 and sum(c['macos'] for c in selected) == 175, 'finite gate scope')
    manifest = json.loads(Path(config['fixture_manifest']).read_text())
    require(manifest['schema'] == 'append-progress-fixtures-v1' and manifest['host'] == 'macos', 'fixture schema')
    require(re.fullmatch(r'append-gate-[a-z0-9-]+', manifest['run']), 'fresh run namespace')
    parent = Path(manifest['parent'])
    require(parent.is_absolute() and parent.resolve(strict=True) == parent and parent.stat().st_mode & 0o777 == 0o700, 'private canonical parent')
    allocations = [(a['case'], a['ordinal'], a['root']) for a in manifest['allocations']]
    wanted = [(c['name'], n, str(parent / f"{manifest['run']}-{c['id']}-{n:02}")) for c in selected for n in range(1, c['macos'] + 1)]
    require(sorted(allocations) == sorted(wanted), 'complete exact fixture assignments')
    require(all(a['version'] == 1 and a['run'] == manifest['run'] for a in manifest['allocations']), 'assignment identity')
    require(all(not Path(root).exists() for _, _, root in allocations), 'fixture collision; preserve')
    pins = config['pins']
    source_manifest = json.loads(Path(config['source_manifest']).read_text())
    def verify():
        for path, expected_hash in pins.items():
            require(digest(path) == expected_hash, f'identity drift: {path}')
        for member in source_manifest['files']:
            require(digest(source / member['path']) == member['sha256'], f"source drift: {member['path']}")
    required = ['nextest', 'cargo_metadata', 'binaries_metadata', 'nextest_config', 'fixture_manifest', 'source_manifest']
    require(all(config[key] in pins for key in required), 'unbound execution input')
    for key in ['POINTBREAK_ENROLLMENT_PREDECESSOR', 'POINTBREAK_ENROLLMENT_GIT']:
        path = config['environment'][key]
        require(Path(path).is_absolute() and pins[path] == config['environment'][key + '_SHA256'], 'predecessor/Git binding')
    verify()
    evidence.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith(('POINTBREAK', 'CARGO', 'RUST', 'NEXTEST'))}
    env.update(config['environment'])
    env['POINTBREAK_APPEND_FIXTURES'] = config['fixture_manifest']
    env['POINTBREAK_APPEND_FIXTURES_SHA256'] = digest(config['fixture_manifest'])
    common = ['--cargo-metadata', config['cargo_metadata'], '--binaries-metadata', config['binaries_metadata'], '--config-file', config['nextest_config'], '--user-config-file', 'none']
    def invoke(label, args):
        folder = evidence / label
        folder.mkdir()
        started = time.time()
        with (folder / 'stdout').open('xb') as out, (folder / 'stderr').open('xb') as err:
            result = subprocess.run([config['nextest'], 'nextest', *args], cwd=source, env=env, stdout=out, stderr=err)
        ended = time.time()
        (folder / 'capture.json').write_text(json.dumps({'arguments': args, 'exit_code': result.returncode, 'started': started, 'ended': ended}) + '\n')
        return folder, result.returncode, started, ended
    folder, code, _, _ = invoke('discovery', ['list', *common, '--message-format', 'json', '--run-ignored', 'all', '-E', 'test(/^session::append_progress::tests::/)'])
    require(code == 0, 'discovery failed; preserve')
    listing = json.loads((folder / 'stdout').read_text())
    actual = [(name, info['ignored']) for suite in listing['rust-suites'].values() for name, info in suite['testcases'].items() if info['filter-match']['status'] == 'matches']
    require(sorted(actual) == sorted((c['name'], c['macos_gate'] or c['child_entry']) for c in expected), 'compiled case/ignore inventory mismatch')
    run_ids = set()
    passed = []
    for case in selected:
        junit = Path(config['junit'])
        require(not junit.exists(), 'uncollected JUnit; preserve')
        folder, code, started, ended = invoke(case['id'], ['run', *common, '--run-ignored', 'all', '--no-tests', 'fail', '--test-threads', '1', '--retries', '0', '--failure-output', 'immediate-final', '--success-output', 'final', '-E', 'test(=' + case['name'] + ')'])
        modified = junit.stat().st_mtime if junit.exists() else None
        report = folder / 'junit.xml'
        if junit.exists():
            shutil.move(junit, report)
        require(code == 0 and modified is not None, f"failed {case['id']}; preserve, no replay")
        xml = ET.parse(report).getroot()
        require(started <= modified <= ended and xml.attrib['uuid'] not in run_ids, 'stale/duplicate result')
        require([t.attrib['name'] for t in xml.iter('testcase')] == [case['name']], 'exact result membership')
        require(not any(xml.iter('failure')) and not any(xml.iter('error')) and not any(xml.iter('skipped')), 'nonpassing selected case')
        run_ids.add(xml.attrib['uuid'])
        passed.append({'name': case['name'], 'junit_sha256': digest(report), 'run_uuid': xml.attrib['uuid']})
        print('PASS', case['id'], flush=True)
    verify()
    (evidence / 'passed.json').write_text(json.dumps({'config_sha256': digest(config_path), 'cases': passed, 'diagnostic_only': True}, indent=2) + '\n')


if __name__ == '__main__':
    main()
