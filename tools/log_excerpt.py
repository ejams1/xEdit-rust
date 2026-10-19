#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Read a bounded JSON excerpt of a log without loading the full file.

Offsets are bytes; use next_offset to continue. The source log is never modified.
"""
import argparse
import codecs
import json
from pathlib import Path
import sys


def positive(value):
    value = int(value)
    if value < 1:
        raise argparse.ArgumentTypeError('must be positive')
    return value


def offset(value):
    value = int(value)
    if value < 0:
        raise argparse.ArgumentTypeError('must not be negative')
    return value


def excerpt(path, start, max_lines, max_chars):
    pieces, positions = [], []
    decoder = codecs.getincrementaldecoder('utf-8')(errors='replace')
    with path.open('rb') as stream:
        stream.seek(0, 2)
        size = stream.tell()
        if start > size:
            raise ValueError('offset is beyond the end of the log')
        stream.seek(start)
        # At most four bytes encode a valid UTF-8 character. A huge line cannot
        # cause an unbounded read, even when it contains no newline.
        window = stream.read(max_chars * 4 + 4)
    lines, character_count = 0, 0
    for index, byte in enumerate(window):
        decoded = decoder.decode(bytes([byte]))
        if decoded:
            # Keep a decoder emission intact: malformed UTF-8 can emit a
            # replacement and the following ASCII character in one step.
            pieces.append(decoded)
            positions.append(start + index + 1)
            character_count += len(decoded)
            lines += decoded.count(chr(10))
        if lines >= max_lines or character_count >= max_chars:
            break
    else:
        if start + len(window) == size:
            remainder = decoder.decode(b'', final=True)
            if remainder:
                pieces.append(remainder)
                positions.append(size)

    def render(count):
        end = positions[count - 1] if count else start
        return json.dumps({'offset': start, 'next_offset': end,
                           'size_bytes': size, 'truncated': end < size,
                           'text': ''.join(pieces[:count])}, ensure_ascii=True)

    # Include JSON escaping and the trailing newline in the output budget.
    low, high = 0, len(pieces)
    while low < high:
        mid = (low + high + 1) // 2
        if len(render(mid)) + 1 <= max_chars:
            low = mid
        else:
            high = mid - 1
    return render(low)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('log', type=Path)
    parser.add_argument('--offset', type=offset, default=0, help='byte offset from a previous next_offset')
    parser.add_argument('--max-lines', type=positive, default=80)
    parser.add_argument('--max-chars', type=positive, default=8000, help='entire JSON output budget, including its newline')
    args = parser.parse_args(argv)
    if args.max_chars < 256:
        parser.error('--max-chars must be at least 256')
    try:
        output = excerpt(args.log, args.offset, args.max_lines, args.max_chars)
        sys.stdout.buffer.write((output + chr(10)).encode('ascii'))
    except (OSError, ValueError) as error:
        message = str(error)
        while len(json.dumps({'error': message}, ensure_ascii=True)) + 1 > args.max_chars:
            message = message[:len(message) // 2]
        output = json.dumps({'error': message}, ensure_ascii=True)
        sys.stderr.buffer.write((output + chr(10)).encode('ascii'))
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
