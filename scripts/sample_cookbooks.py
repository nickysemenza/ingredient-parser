#!/usr/bin/env python3
"""Reproduce a book-disjoint sample from publisher-tagged EPUB ingredient lines.

Only the standard library is needed. No parser output, model calls, or confidence
heuristics enter selection. --verify checks committed source records against the
local EPUBs without rewriting anything. Copyrighted books stay outside the repo.
"""
import argparse
import hashlib
import json
from pathlib import Path
import xml.etree.ElementTree as ET
import zipfile

CORPUS = Path(__file__).resolve().parents[1] / 'ingredient-parser/tests/corpus'
SEED = '2026-09-07-architecture-v1'
BOOKS = [
    ('wok', 'development', 'The Everyday Wok Cookbook -', ['bl_hanging']),
    ('arabiyya', 'development', 'Arabiyya_ Recipes from the Life', ['ril', 'rilf', 'r1il', 'r1ilf', 'rils']),
    ('charred', 'development', 'Charred -', ['hang']),
    ('home-kitchen', 'development', 'At Home in the Kitchen -', ['ril', 'rilf']),
    ('bakers-companion', 'development', 'The King Arthur Flour All-Purpose Baker', ['ing', 'ing1']),
    ('nopalito', 'development', 'Nopalito -', ['ril']),
    ('rintaro', 'holdout', 'Rintaro -', ['hang', 'hang-1']),
    ('honey-co', 'holdout', 'Honey & Co._ The Cookbook -', ['ingredient', 'ingredient1']),
    ('dining-in', 'holdout', 'Dining In -', ['ril', 'rilf', 'rils']),
    ('six-seasons', 'holdout', 'Six Seasons -', ['RI', 'RIB']),
]


def normalize(text):
    return ' '.join(text.split())


def is_section_heading(element):
    """Reject publisher-styled headings embedded in ingredient containers."""
    return any(
        'underline' in descendant.attrib.get('class', '').split()
        for descendant in element.iter()
    )


def sample(root):
    files = sorted(root.rglob('*.epub'))
    # The frozen exclusion list prevents overlap with the pre-redesign corpus.
    excluded = set(json.loads((CORPUS / 'cookbooks/excluded-inputs.json').read_text()))
    records, manifest = [], []
    for book_id, split, filename_prefix, classes in BOOKS:
        matches = [p for p in files if p.name.startswith(filename_prefix)]
        if len(matches) != 1:
            raise ValueError(f'{book_id}: expected one EPUB, found {matches}')
        path = matches[0]
        candidates = {}
        with zipfile.ZipFile(path) as archive:
            for href in sorted(archive.namelist()):
                if not href.endswith(('.html', '.xhtml', '.htm')):
                    continue
                try:
                    document = ET.fromstring(archive.read(href))
                except ET.ParseError:
                    continue
                for index, element in enumerate(document.iter()):
                    if element.tag.rsplit('}', 1)[-1] not in ('p', 'li'):
                        continue
                    if not set(element.attrib.get('class', '').split()) & set(classes):
                        continue
                    if is_section_heading(element):
                        continue
                    text = normalize(''.join(element.itertext()))
                    key = text.casefold()
                    if not text or text.endswith(':') or key in excluded:
                        continue
                    candidates.setdefault(key, {
                        'book_id': book_id, 'split': split, 'input': text,
                        'source': {'href': href, 'element_index': index,
                                   'element_id': element.attrib.get('id')},
                    })
        def rank(row):
            return hashlib.sha256(f'{SEED}\0{book_id}\0{row["input"]}'.encode()).hexdigest()
        selected = sorted(candidates.values(), key=rank)[:50]
        if len(selected) != 50:
            raise ValueError(f'{book_id}: fewer than 50 candidate lines')
        for index, row in enumerate(selected, 1):
            records.append({'id': f'{book_id}-{index:02d}', **row})
        manifest.append({'book_id': book_id, 'split': split,
                         'epub': str(path.relative_to(root)),
                         'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
                         'ingredient_classes': classes, 'candidate_count': len(candidates),
                         'sample_size': len(selected)})
    return records, {'seed': SEED, 'method': 'sha256-ranked distinct publisher ingredient elements',
                     'excluded': 'excluded-inputs.json', 'books': manifest}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('library', type=Path)
    parser.add_argument('--verify', action='store_true')
    args = parser.parse_args()
    rows, manifest = sample(args.library)
    out = CORPUS / 'cookbooks'
    values = {
        'sources.jsonl': ''.join(json.dumps(row, ensure_ascii=False) + '\n' for row in rows),
        'manifest.json': json.dumps(manifest, ensure_ascii=False, indent=2) + '\n',
    }
    for name, text in values.items():
        if args.verify:
            if (out / name).read_text() != text:
                raise ValueError(f'{name} differs from reproducible source sample')
        else:
            (out / name).write_text(text)
    benchmark = CORPUS.parent.parent / 'benches/cookbook-lines.txt'
    text = ''.join(row['input'] + '\n' for row in rows if row['split'] == 'development')
    if args.verify:
        if benchmark.read_text() != text:
            raise ValueError('benchmark input differs from development sample')
    else:
        benchmark.write_text(text)
    print(f'{"Verified" if args.verify else "Sampled"} {len(rows)} ingredient lines from {len(manifest["books"])} books')


if __name__ == '__main__':
    main()
