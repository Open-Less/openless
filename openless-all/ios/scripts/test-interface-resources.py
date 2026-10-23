#!/usr/bin/env python3
"""Read-only source/resource audit; does not prove runtime localization coverage."""
import json
from pathlib import Path
import re
import unittest

IOS = Path(__file__).resolve().parents[1]
STRING = re.compile(r'"(?:\\.|[^"\\])*"')
HAN = re.compile(r'[\u3400-\u9fff]')


def read_strings(language):
    result = {}
    path = IOS / 'Resources' / f'{language}.lproj' / 'Localizable.strings'
    for number, line in enumerate(path.read_text().splitlines(), 1):
        if not line.strip() or line.startswith('//'):
            continue
        match = re.fullmatch(r'("(?:\\.|[^"\\])*") = ("(?:\\.|[^"\\])*");', line)
        if not match:
            raise AssertionError(f'{path}:{number}: invalid strings entry')
        key, value = map(json.loads, match.groups())
        if key in result:
            raise AssertionError(f'{path}:{number}: duplicate key {key}')
        result[key] = value
    return result


class InterfaceResourceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.zh = read_strings('zh-Hans')
        cls.en = read_strings('en')
        cls.inventory = json.loads((IOS / 'Resources/LocalizationSources.json').read_text())['entries']

    def test_resource_and_inventory_keys_match(self):
        self.assertEqual(self.zh.keys(), self.en.keys())
        self.assertEqual(self.zh.keys(), self.inventory.keys())
        self.assertGreater(len(self.zh), 500)

    def test_translations_are_nonempty_and_english_is_not_a_chinese_placeholder(self):
        for key in self.zh:
            with self.subTest(key=key):
                self.assertTrue(self.zh[key].strip())
                self.assertTrue(self.en[key].strip())
                self.assertFalse(HAN.search(self.en[key]))

    def test_format_placeholders_match_and_have_helpers(self):
        helper = (IOS / 'Packages/OpenLessShared/Sources/OpenLessShared/InterfaceLocalizer.swift').read_text()
        for key in self.zh:
            if key.startswith('interface.'):
                with self.subTest(key=key):
                    self.assertEqual(re.findall(r'%(?:ld|@)', self.zh[key]), re.findall(r'%(?:ld|@)', self.en[key]))
                    self.assertIn(f'"{key}"', helper)

    def test_current_chinese_static_literals_have_resources(self):
        # Interpolated strings use explicit format keys; this checks static literals.
        paths = [*IOS.glob('App/**/*.swift'), *IOS.glob('Keyboard/**/*.swift'),
                 *IOS.glob('PlatformShared/**/*.swift'),
                 IOS / 'Packages/OpenLessShared/Sources/OpenLessShared/Contract.swift']
        for path in paths:
            for number, line in enumerate(path.read_text().splitlines(), 1):
                if line.lstrip().startswith('//'):
                    continue
                for token in STRING.findall(line):
                    if HAN.search(token) and '\\(' not in token:
                        key = json.loads(token)
                        with self.subTest(source=f'{path.relative_to(IOS)}:{number}', key=key):
                            self.assertIn(key, self.zh)

    def test_desktop_translations_have_traceable_sources(self):
        reused = [entry for entry in self.inventory.values() if entry['translationSource'].startswith('app/')]
        self.assertGreaterEqual(len(reused), 60)
        for entry in reused:
            self.assertTrue(entry['translationSource'].startswith('app/src/i18n/{zh-CN,en}.ts:'))


if __name__ == '__main__':
    unittest.main(verbosity=2)
