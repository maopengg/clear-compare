#!/usr/bin/env python3
"""Refresh resolved dependency declarations and append missing packaged notices.

Usage: cargo metadata --locked --format-version 1 > target/metadata.json
       python3 scripts/update-licenses.py target/metadata.json
"""
import json
import re
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
packages = sorted((p for p in json.loads(Path(sys.argv[1]).read_text())['packages']
                   if p.get('source')), key=lambda p: (p['name'], p['version']))
header = ('# Third-party dependencies\n\nGenerated from Cargo.lock and cargo metadata. '
          'This lists the full resolved workspace, including platform-specific packages. '
          'Actual bundled dependencies depend on build targets and features. Original Clear Compare '
          'code is MIT; these packages retain their stated licenses.\n\n'
          '| Package | Version | Declared license | Source |\n|---|---|---|---|\n')
rows = []
notices_path = root / 'THIRD_PARTY_NOTICES.md'
notices = notices_path.read_text()
for p in packages:
    name, version = p['name'], p['version']
    license_text = (p.get('license') or 'See upstream license').replace('|', '&#124;')
    rows.append(f'| {name} | {version} | {license_text} | [crate](https://crates.io/crates/{name}/{version}) |\n')
    title = f'## {name} {version}'
    if re.search(r'^' + re.escape(title) + r'$', notices, re.MULTILINE):
        continue
    base = Path(p['manifest_path']).resolve().parent
    candidates = [f for f in base.rglob('*') if f.is_file() and
                  f.name.lower().startswith(('license', 'copying', 'copyright', 'notice'))]
    if p.get('license_file'):
        candidates.append(base / p['license_file'])
    texts = []
    for f in sorted(set(candidates)):
        resolved = f.resolve()
        if base not in resolved.parents or not resolved.is_file() or resolved.stat().st_size > 1024 * 1024:
            continue
        try:
            text = resolved.read_text(encoding='utf-8')
        except UnicodeError:
            continue
        texts.append(f'### {f.relative_to(base).as_posix()}\n\n{text}\n')
    notices += '\n' + title + '\n\n' + ('\n'.join(texts) or
               'No license text was packaged; see the upstream source and declaration.\n')
(root / 'docs/third-party-dependencies.md').write_text(header + ''.join(rows))
notices_path.write_text(notices.rstrip() + chr(10))
