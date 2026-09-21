"""Notice reduction must preserve copyright and associate it with the right terms."""
import unittest

from generate_notices import render


class NoticeTests(unittest.TestCase):
    def test_shared_terms_retain_each_copyright_and_component(self):
        terms = 'Permission is hereby granted to use this software.\nTHE SOFTWARE IS PROVIDED AS IS.'
        result = render([
            ('MIT', 'Copyright Alice\n\n' + terms, {'package-a'}),
            ('MIT', 'Copyright Bob\n\n' + terms.replace('\n', ' '), {'package-b'}),
            ('MIT', 'Copyright Alice\n\n' + terms, {'package-a'}),
        ], 'test')
        self.assertEqual(result.count('Permission is hereby granted'), 1)
        self.assertEqual(result.count('Copyright Alice'), 1)
        self.assertIn('Components: package-a\nCopyright Alice', result)
        self.assertIn('Components: package-b\nCopyright Bob', result)
        self.assertIn('THE SOFTWARE IS PROVIDED AS IS.', result)

    def test_different_terms_and_trailing_notices_are_not_lost(self):
        original = 'Permission is hereby granted with condition A.\nCopyright after the terms'
        changed = 'Permission is hereby granted with condition B.'
        result = render([('MIT', original, {'a'}), ('MIT', changed, {'b'})], 'test')
        self.assertIn(original, result)
        self.assertIn(changed, result)
        self.assertEqual(result.count('Permission is hereby granted'), 2)


if __name__ == '__main__':
    unittest.main()
