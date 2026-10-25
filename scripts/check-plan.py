#!/usr/bin/env python3
"""Check the plan (ROADMAP.md, plan/, README.md, AGENTS.md, docs/) against itself.

- The ROADMAP's milestone table, each card's slice table and the backlog's
  tables agree with the files they list: every file has one row, every row's
  file exists, and the statuses match.
- Every card, slice and backlog entry has a `Status:` line on its third line.
- Every relative link in a Markdown file resolves, anchors included
  (`plan/complete/` is history: only its links into the live plan count).
- No file outgrows its size budget.

Run from anywhere: `just plan-check`. Exits 1 on any problem.
"""
import os
import re
import subprocess
import sys

ROOT = subprocess.run(['git', 'rev-parse', '--show-toplevel'], capture_output=True, text=True,
                      cwd=os.path.dirname(os.path.abspath(__file__))).stdout.strip()
os.chdir(ROOT)

SLICE_STATUS = {'open', 'in progress', 'done', 'deferred', 'blocked'}
CARD_STATUS = {'planned', 'sketched', 'paused', 'deferred', 'in progress', 'proposed'}
KB = 1024
BUDGETS = [  # first match wins
    (re.compile(r'^plan/complete/'), None),
    (re.compile(r'^ROADMAP\.md$'), 12 * KB),
    (re.compile(r'^AGENTS\.md$'), 8 * KB),
    (re.compile(r'^README\.md$'), 16 * KB),
    (re.compile(r'^plan/backlog/README\.md$'), 16 * KB),
    (re.compile(r'^plan/[^/]+/README\.md$'), 12 * KB),
    (re.compile(r'^plan/'), 24 * KB),
]

errors = []


def err(path, msg):
    errors.append(f'{path}: {msg}')


def read(p):
    with open(p, encoding='utf-8') as f:
        return f.read()


def slug(h):
    h = re.sub(r'^#+\s*', '', h).strip().lower()
    h = re.sub(r'[^\w\- ]', '', h, flags=re.UNICODE)
    return h.replace(' ', '-')


_anchors = {}


def anchors(p):
    if p not in _anchors:
        seen, out, fence = {}, set(), False
        for line in read(p).split('\n'):
            if line.lstrip().startswith('```'):
                fence = not fence
            elif not fence and re.match(r'^#{1,6} ', line):
                s = slug(line)
                n = seen.get(s, 0)
                seen[s] = n + 1
                out.add(s if n == 0 else f'{s}-{n}')
            for m in re.finditer(r'<a (?:name|id)="([^"]+)"', line):
                out.add(m.group(1))
        _anchors[p] = out
    return _anchors[p]


def status(p):
    """The word(s) after `Status:` on the third line, or None."""
    lines = read(p).split('\n')
    m = re.match(r'^Status: ([a-z]+(?: [a-z]+)?)\b', lines[2] if len(lines) > 2 else '')
    return m.group(1) if m else None


LINK = re.compile(r'\[([^\]]*)\]\(([^)\s]+)\)')


def links(p):
    """(text, target) for each Markdown link outside code."""
    fence = False
    for line in read(p).split('\n'):
        if line.lstrip().startswith('```'):
            fence = not fence
            continue
        if fence:
            continue
        line = re.sub(r'`[^`]*`', '', line)
        for m in LINK.finditer(line):
            yield m.group(1), m.group(2)


def resolve(p, target):
    """The repo path a relative link points at, and its anchor."""
    path, _, anchor = target.partition('#')
    full = os.path.normpath(os.path.join(os.path.dirname(p), path)) if path else p
    return full, anchor


def tracked_md():
    out = subprocess.run(['git', 'ls-files', '-co', '--exclude-standard', '*.md'],
                         capture_output=True, text=True).stdout.split()
    return [p for p in out if os.path.isfile(p) and not p.startswith(('vendor/', 'target/'))]


def check_links(files):
    for p in files:
        history = p.startswith('plan/complete/')
        for text, target in links(p):
            if re.match(r'[a-z][a-z0-9+.-]*:', target):
                continue
            full, anchor = resolve(p, target)
            if history and not full.startswith(('plan/', 'ROADMAP.md', 'AGENTS.md')):
                continue
            if history and full.startswith('plan/complete/'):
                continue
            if full.startswith('..') or not os.path.exists(full):
                err(p, f'broken link: {target}')
            elif anchor and full.endswith('.md') and os.path.isfile(full) and anchor not in anchors(full):
                err(p, f'no such anchor: {target}')


def check_budgets(files):
    for p in files:
        for pat, limit in BUDGETS:
            if pat.search(p):
                if limit and os.path.getsize(p) > limit:
                    err(p, f'{os.path.getsize(p)} bytes, over its budget of {limit // KB} KB')
                break


def table_rows(text, first_col):
    """Rows of the Markdown tables whose first cell is a link, as (link text,
    target, cells)."""
    for line in text.split('\n'):
        if not line.startswith('| ['):
            continue
        cells = [c.strip() for c in line.strip().strip('|').split(' | ')]
        m = re.match(r'^\[([^\]]+)\]\(([^)]+)\)$', cells[0])
        if m:
            yield m.group(1), m.group(2), cells


def check_cards():
    cards = sorted(f'plan/{d}/README.md' for d in os.listdir('plan')
                   if re.match(r'm\d', d) and os.path.isfile(f'plan/{d}/README.md'))
    for card in cards + ['plan/between/README.md']:
        st = status(card)
        if st not in CARD_STATUS:
            err(card, f'third line should be `Status: <{"|".join(sorted(CARD_STATUS))}> …`, not {st!r}')
    for card in cards:
        d = os.path.dirname(card)
        listed = set()
        for label, target, cells in table_rows(read(card), 'Slice'):
            full, _ = resolve(card, target)
            listed.add(full)
            if not os.path.isfile(full):
                err(card, f'slice {label}: no file {target}')
                continue
            st = status(full)
            if st not in SLICE_STATUS:
                err(full, f'third line should be `Status: <{"|".join(sorted(SLICE_STATUS))}> …`, not {st!r}')
            if len(cells) < 3 or cells[2] != st:
                err(card, f'slice {label} says {cells[2] if len(cells) > 2 else "?"!r}; its file says {st!r}')
            if (st == 'done') != full.startswith('plan/complete/'):
                err(card, f'slice {label} is {st!r} but its file is {"not " if st == "done" else ""}in plan/complete/')
        sd = f'{d}/slices'
        if os.path.isdir(sd):
            for f in sorted(os.listdir(sd)):
                if f'{sd}/{f}' not in listed:
                    err(card, f'no row for slices/{f}')


def check_roadmap():
    text = read('ROADMAP.md')
    seen = set()
    for label, target, cells in table_rows(text, '#'):
        full, _ = resolve('ROADMAP.md', target)
        if not re.match(r'M[\d.]+$', label):
            continue
        seen.add(full)
        if not os.path.isfile(full):
            err('ROADMAP.md', f'{label}: no file {target}')
            continue
        row = cells[3] if len(cells) > 3 else ''
        if full.startswith('plan/complete/'):
            if row != 'Done':
                err('ROADMAP.md', f'{label} links to plan/complete/ but its status is {row!r}, not Done')
        else:
            st = status(full)
            if not row.lower().startswith(st or '?'):
                err('ROADMAP.md', f'{label}: status {row!r} should start with its card\'s {st!r}')
    for d in os.listdir('plan'):
        card = f'plan/{d}/README.md'
        if re.match(r'm\d', d) and os.path.isfile(card) and card not in seen:
            err('ROADMAP.md', f'no row for {card}')


def check_backlog():
    d = 'plan/backlog'
    text = read(f'{d}/README.md')
    rows = {}
    for label, target, cells in table_rows(text, 'ID'):
        if label in rows:
            err(f'{d}/README.md', f'{label} has two rows')
        rows[label] = (target, cells)
    files = {f[:-3] for f in os.listdir(d) if re.match(r'^[BF]\d+\.md$', f)}
    closed = set(re.findall(r'(?m)^### ([BF]\d+):', read('plan/complete/backlog-closed.md')))
    for i in sorted(files - set(rows)):
        err(f'{d}/README.md', f'no row for {i}.md')
    for i, (target, cells) in sorted(rows.items()):
        if target != f'{i}.md':
            err(f'{d}/README.md', f'{i} links to {target}, not {i}.md')
        if i not in files:
            err(f'{d}/README.md', f'{i} has a row but no file')
            continue
        p = f'{d}/{i}.md'
        title = read(p).split('\n', 1)[0]
        m = re.match(rf'^# {i}: (.+)$', title)
        if not m:
            err(p, f'first line should be `# {i}: <title>`')
        elif len(cells) > 1 and cells[1] != m.group(1).replace('|', '/'):
            err(f'{d}/README.md', f'{i}: title {cells[1]!r} differs from its file\'s {m.group(1)!r}')
        st = status(p)
        if st not in SLICE_STATUS - {'done'}:
            err(p, f'third line should be `Status: open` (or in progress, deferred, blocked), not {st!r}')
    for i in sorted(files & closed):
        err(f'{d}/{i}.md', 'is open, but plan/complete/backlog-closed.md closes it too')


def check_no_todos():
    out = subprocess.run(['git', 'ls-files', '-co', '--exclude-standard'], capture_output=True, text=True).stdout.split()
    for p in out:
        if p.startswith(('vendor/', 'plan/complete/', 'target/')) or not os.path.isfile(p):
            continue
        if p in ('scripts/check-plan.py',):
            continue
        try:
            text = read(p)
        except (UnicodeDecodeError, IsADirectoryError):
            continue
        for m in re.finditer(r'(?<![\w/.-])todos/[\w./-]+', text):
            # Two memos that were only ever on other branches.
            if re.match(r'todos/(eval-gpui-kit|table-extraction)\.md', m.group(0)):
                continue
            err(p, f'refers to {m.group(0)}; the plan moved to plan/')


def main():
    files = tracked_md()
    check_links(files)
    check_budgets(files)
    check_cards()
    check_roadmap()
    check_backlog()
    check_no_todos()
    for e in errors:
        print(e)
    if errors:
        print(f'\n{len(errors)} problem(s) in the plan', file=sys.stderr)
        sys.exit(1)
    print('plan: ok')


main()
