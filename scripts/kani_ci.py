#!/usr/bin/env python3
"""Shared harness discovery, deterministic CI planning and result gate (stdlib only)."""
import argparse
import collections
import csv
import json
import os
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent.parent
SOURCES = {'colada-engine': 'tests/kani*.rs', 'colada': 'percolator/tests/kani*.rs'}


def harnesses(package):
    proof = False
    names = set()
    for path in sorted(ROOT.glob(SOURCES[package])):
        for line in path.read_text().splitlines():
            if '#[kani::proof]' in line:
                proof = True
            match = re.match(r'\s*fn ([a-zA-Z0-9_]+)\(', line)
            if proof and match:
                if match[1] in names:
                    raise ValueError(f'Duplicate harness name: {match[1]}')
                names.add(match[1])
                proof = False
    attributes = sum(path.read_text().count('#[kani::proof]') for path in ROOT.glob(SOURCES[package]))
    if not names or attributes != len(names):
        raise ValueError(f'{package}: {attributes} proof attributes but {len(names)} named harnesses; '
                         'declare every instance as a literal `fn name(`')
    return sorted(names)


def recorded():
    with (ROOT / 'verification/kani-results.tsv').open() as f:
        return {(r['package'], r['harness']): r for r in csv.DictReader(f, delimiter='\t')}


def classify(code, log):
    # Expected-panic controls can contain failed checks yet succeed as Kani runs.
    if code == 0:
        return 'PASS'
    # A failed check printed before a timeout or abort is still a counterexample.
    failed_check = re.search(r'Status: FAILURE|Failed Checks:|\*\* [1-9][0-9]* of [0-9]+ failed', log)
    if failed_check:
        return 'FAIL'
    if code in (124, 137):
        return 'TIMEOUT'
    # CBMC can print FAILED after aborting without producing a counterexample.
    resource = re.search(r'too many addressed objects|out of memory|bad_alloc|memory exhausted|cannot allocate memory|CBMC failed with status [^0\s]', log, re.I)
    if resource:
        return 'INCONCLUSIVE'
    if 'VERIFICATION:- FAILED' in log:
        return 'FAIL'
    return 'INCONCLUSIVE'


def plan(count, only=''):
    if count < len(SOURCES):
        raise ValueError('Need at least one shard per package')
    baseline = recorded()
    work = {p: [(h, float(baseline.get((p, h), {}).get('seconds') or 120))
                for h in harnesses(p) if re.search(only, h)] for p in SOURCES}
    work = {p: w for p, w in work.items() if w}
    if not work:
        raise ValueError('The filter selects no harness')
    counts = dict.fromkeys(work, 1)
    for _ in range(count - len(work)):
        choices = [p for p in work if counts[p] < len(work[p])]
        if not choices:
            break
        p = max(choices, key=lambda p: sum(t for _, t in work[p]) / counts[p])
        counts[p] += 1
    matrix = []
    for p in work:
        bins = [{'package': p, 'shard': i, 'harnesses': [], 'seconds': 0}
                for i in range(counts[p])]
        for h, seconds in sorted(work[p], key=lambda item: (-item[1], item[0])):
            bucket = min(bins, key=lambda b: (b['seconds'], b['shard']))
            bucket['harnesses'].append(h)
            bucket['seconds'] += seconds
        matrix.extend(bins)
    return {'include': matrix}


def gate(directory, only=''):
    baseline = recorded()
    actual = {}
    invalid = False
    for path in sorted(Path(directory).rglob('results.tsv')):
        package = path.parent.name
        if package not in SOURCES:
            raise ValueError(f'Unknown result package: {path}')
        with path.open() as f:
            for row in csv.DictReader(f, delimiter='\t'):
                key = (package, row['proof'])
                if key in actual:
                    raise ValueError(f'Duplicate result: {key}')
                actual[key] = row
                invalid |= row['status'] == 'FAIL'
    lines = ['| Package | Harness | Expected status | CI status | Seconds |',
             '| --- | --- | --- | --- | --- |']
    totals = collections.Counter()
    for p in SOURCES:
        for h in harnesses(p):
            if not re.search(only, h):
                continue
            expected = baseline.get((p, h), {}).get('status', 'UNRECORDED')
            row = actual.get((p, h), {})
            status = row.get('status', 'MISSING')
            invalid |= status in ('FAIL', 'MISSING') or (expected == 'PASS' and status != 'PASS')
            totals[status] += 1
            lines.append(f"| {p} | {h} | {expected} | {status} | {row.get('time_s', '—')} |")
    if not totals:
        raise ValueError('The filter selects no harness')
    lines.extend(['', 'Totals: ' + ', '.join(f'{s}: {n}' for s, n in sorted(totals.items()))])
    summary = '\n'.join(lines) + '\n'
    print(summary)
    if os.environ.get('GITHUB_STEP_SUMMARY'):
        with open(os.environ['GITHUB_STEP_SUMMARY'], 'a') as f:
            f.write(summary)
    return int(invalid)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('list').add_argument('package', choices=SOURCES)
    planner = sub.add_parser('plan')
    planner.add_argument('--shards', type=int, default=16)
    planner.add_argument('--only', default='', help='regex selecting harness names')
    c = sub.add_parser('classify')
    c.add_argument('code', type=int)
    c.add_argument('log', type=Path)
    gater = sub.add_parser('gate')
    gater.add_argument('directory')
    gater.add_argument('--only', default='')
    args = parser.parse_args()
    if args.command == 'list':
        print('\n'.join(harnesses(args.package)))
    elif args.command == 'plan':
        print(json.dumps(plan(args.shards, args.only)))
    elif args.command == 'classify':
        print(classify(args.code, args.log.read_text(errors='replace')))
    else:
        return gate(args.directory, args.only)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
