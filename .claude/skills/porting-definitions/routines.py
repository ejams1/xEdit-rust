"""Prints the Pascal routines of a unit by name: routines.py <unit> <name>..."""
import re
import sys

path, names = sys.argv[1], sys.argv[2:]
text = open(path, encoding='cp1252', errors='replace').read()
lines = text.split('\n')
impl_start = next(i for i, line in enumerate(lines) if line.strip().lower() == 'implementation')
wanted = {n.lower() for n in names}
found = set()
i = impl_start
while i < len(lines):
    m = re.match(r'(function|procedure)\s+(\w+)\s*[(:;]', lines[i])
    if m and m.group(2).lower() in wanted:
        start = i
        end = start
        while end < len(lines) and not re.match(r'end;\s*$', lines[end]):
            end += 1
        print('\n'.join(lines[start:end + 1]))
        print()
        found.add(m.group(2).lower())
        i = end + 1
    else:
        i += 1
for name in names:
    if name.lower() not in found:
        print(f'// {name}: not found')
