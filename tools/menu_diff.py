#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Compare the upstream Delphi popup menus with the port's menu model.

The `.dfm` of the released 4.1.5q build is the Delphi build's own menu
definition, so a caption-by-caption tree diff of every popup menu is the
menu walkthrough against that build: the caption of every item, the
nesting, and the `Enabled`, `Checked` and `RadioItem` flags.

    python tools/menu_diff.py --dfm <xeMainForm.dfm> [--model <menu_model.rs>]

Exits 0 when the two agree and 1 when they do not.
"""
import argparse
import re
import sys

MENUS = [
    "pmuMain", "pmuNav", "pmuView", "pmuSpreadsheet", "pmuViewHeader",
    "pmuNavHeaderPopup", "pmuRefBy", "pmuNavAdd", "pmuMessages", "pmuBtnMenu",
]


def balanced(text, open_idx, opener="{", closer="}"):
    """Return the index just past the block that starts at open_idx."""
    depth = 0
    i = open_idx
    while i < len(text):
        if text[i] == opener:
            depth += 1
        elif text[i] == closer:
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    raise ValueError("unbalanced block at %d" % open_idx)


def dfm_menu(text, name):
    """Return the items of a popup menu of the .dfm."""
    start = re.search(r"^  object %s: TPopupMenu$" % name, text, re.M)
    if not start:
        return None
    body = text[start.end():]
    end = re.search(r"^  object \w+: ", body, re.M)
    body = body[:end.start()] if end else body
    items = []
    stack = [(0, items)]
    cur = None
    for line in body.splitlines():
        m = re.match(r"^(\s*)object (\w+): TMenuItem$", line)
        if m:
            # an item of the popup is at indent 4, its children at 6, 8, ...
            depth = (len(m.group(1)) - 4) // 2
            cur = {"caption": None, "enabled": True, "checked": False,
                   "radio": False, "visible": True, "subs": []}
            while len(stack) <= depth:
                stack.append((len(stack) - 1, []))
            stack[depth][1].append(cur)
            stack = stack[:depth + 1] + [(depth + 1, cur["subs"])]
            continue
        if cur is None:
            continue
        m = re.match(r"^\s*Caption = '(.*)'$", line)
        if m:
            cur["caption"] = m.group(1)
        elif re.match(r"^\s*Enabled = False$", line):
            cur["enabled"] = False
        elif re.match(r"^\s*Checked = True$", line):
            cur["checked"] = True
        elif re.match(r"^\s*RadioItem = True$", line):
            cur["radio"] = True
        elif re.match(r"^\s*Visible = False$", line):
            cur["visible"] = False
    return items


def rs_items(text, open_idx):
    """Parse the `&[ ... ]` item list that starts at open_idx."""
    if text[open_idx] != "[":
        raise ValueError("not an item list at %d" % open_idx)
    end = balanced(text, open_idx, "[", "]")
    body = text[open_idx + 1:end - 1]
    items = []
    pos = 0
    while True:
        m = re.search(r"MenuItem\s*\{", body[pos:])
        if not m:
            break
        start = pos + m.end() - 1
        blk_end = balanced(body, start)
        block = body[start + 1:blk_end - 1]
        pos = blk_end
        it = {"caption": None, "enabled": True, "checked": False,
              "radio": False, "visible": True, "subs": []}
        c = re.search(r'caption:\s*"((?:[^"\\]|\\.)*)"', block)
        if c:
            it["caption"] = c.group(1)
        e = re.search(r"enabled:\s*(true|false)", block)
        if e:
            it["enabled"] = e.group(1) == "true"
        k = re.search(r"checked:\s*(true|false)", block)
        if k:
            it["checked"] = k.group(1) == "true"
        r = re.search(r"radio:\s*(true|false)", block)
        if r:
            it["radio"] = r.group(1) == "true"
        v = re.search(r"visible:\s*(true|false)", block)
        if v:
            it["visible"] = v.group(1) == "true"
        ch = re.search(r"children:\s*&\[", block)
        if ch:
            it["subs"] = rs_items(block, ch.end() - 1)
        items.append(it)
    return items


def rs_menu(text, const):
    """The items of a menu of the port's model."""
    start = re.search(r"pub const %s: MenuTree = MenuTree \{" % const, text)
    if not start:
        return None
    body = text[start.end():]
    end = re.search(r"^};$", body, re.M)
    body = body[:end.start()] if end else body
    m = re.search(r"items:\s*&\[", body)
    if not m:
        return []
    return rs_items(body, m.end() - 1)


def norm(caption):
    """The caption without its accelerator, as the two sides are compared."""
    if caption is None:
        return "<none>"
    caption = caption.replace('\\"', '"').replace("\\\\", "\\")
    return caption.replace("&", "").strip()


def tree(items, depth=0, out=None):
    out = [] if out is None else out
    for it in items:
        out.append((depth, it))
        tree(it["subs"], depth + 1, out)
    return out


def const_of(menu):
    return "PMU_" + re.sub(r"(?<!^)(?=[A-Z])", "_", menu[3:]).upper()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dfm", required=True, help="the upstream xeMainForm.dfm")
    parser.add_argument("--model", default="crates/xedit-gui/src/menu_model.rs",
                        help="the port's menu model")
    args = parser.parse_args()

    dfm = open(args.dfm, encoding="utf-8", errors="replace").read()
    rs = open(args.model, encoding="utf-8").read()
    problems = 0
    for name in MENUS:
        d = dfm_menu(dfm, name)
        r = rs_menu(rs, const_of(name))
        if d is None:
            print("%s: missing in the dfm" % name)
            problems += 1
            continue
        if r is None:
            print("%s: missing in the port (const %s)" % (name, const_of(name)))
            problems += 1
            continue
        dt, rt = tree(d), tree(r)
        ds = [(depth, norm(i["caption"])) for depth, i in dt]
        rs_ = [(depth, norm(i["caption"])) for depth, i in rt]
        if ds != rs_:
            problems += 1
            print("== %s: TREE DIFFERS" % name)
            for i in range(max(len(ds), len(rs_))):
                a = ("  " * ds[i][0] + ds[i][1]) if i < len(ds) else "(none)"
                b = ("  " * rs_[i][0] + rs_[i][1]) if i < len(rs_) else "(none)"
                mark = "  " if a == b else "**"
                print("   %s %-46s | %s" % (mark, a, b))
        else:
            print("== %s: %d top-level, %d in all (dfm == port)" % (
                name, len(d), len(ds)))
        for (da, i), (ra, j) in zip(dt, rt):
            if norm(i["caption"]) != norm(j["caption"]):
                continue
            for field in ("enabled", "checked", "radio", "visible"):
                if i[field] != j[field]:
                    problems += 1
                    print("   ** %s differs: %s dfm=%s port=%s" % (
                        field, norm(i["caption"]), i[field], j[field]))
    print("\nproblems: %d" % problems)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
