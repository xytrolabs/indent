#!/usr/bin/env python3
"""Stage 1 of the copy-on-write `Value` migration — NOT a complete change.

`Value::List(store)` / `Value::Dict(store)` / `Value::Set(store)` become
`Value::X(Arc::new(store))` — but ONLY in expression position. Match/`if let`
patterns (`Value::List(v) => ...`, `if let Value::List(v) = x`) must stay as they
are, because there the variant is being destructured, not built.

Applying this ALONE leaves the tree broken: with `Arc<Vec<Value>>` the ~137
destructuring sites plus every mutation site need a decision (borrow and read it?
`Arc::make_mut` to write? `Arc::try_unwrap` to take ownership?), and our first
attempt produced **138 compile errors**. Do the whole migration in one sitting,
with the full test suite plus `tests/append_semantics.ind` and
`tests/index_write_semantics.ind` to prove copy semantics survive.

Usage:
    python3 tools/arc_rewrite.py --dry-run     # report only (safe)
    python3 tools/arc_rewrite.py               # apply (tree will not compile yet)
"""
import re
import sys

SRC = "indent-native/src/main.rs"
VARIANTS = ["Value::List", "Value::Dict", "Value::Set"]

# Arguments we know how to wrap safely: a plain identifier, or vec![...]/X::new()
IDENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


def find_close_paren(text: str, open_idx: int) -> int:
    """Index of the ')' matching the '(' at open_idx (no strings expected here)."""
    depth = 0
    for i in range(open_idx, len(text)):
        c = text[i]
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return i
    return -1


def is_pattern_position(before: str, after: str) -> bool:
    """Is this occurrence a *pattern* (destructuring) rather than a construction?

    Only the text immediately following the closing paren is decisive for match
    arms (`Value::List(v) => ...`, `if let Value::List(v) = x`). Looking at what
    precedes it is unreliable: `x => Ok(Value::List(v))` is a *construction* that
    happens to sit after `=>` on the same line.
    """
    before = before.rstrip()
    after = after.lstrip()
    if after.startswith("=>") or after.startswith(" if "):
        return True
    # `if let Value::List(v) = x` / `while let ... = x` (but not `==`)
    if after.startswith("=") and not after.startswith("=="):
        return True
    # Alternation inside a pattern: `matches!(v, Value::List(_) | Value::List(_))`
    if after.startswith("|"):
        return True
    if before.endswith("let") or before.endswith("match"):
        return True
    return False


def rewrite(text: str, apply: bool):
    edits = []      # (start, end, replacement)
    skipped = []    # human-readable notes
    for variant in VARIANTS:
        for m in re.finditer(re.escape(variant) + r"\(", text):
            open_idx = m.end() - 1
            close_idx = find_close_paren(text, open_idx)
            if close_idx < 0:
                skipped.append(("no close paren", text[m.start():m.start() + 60]))
                continue
            arg = text[open_idx + 1:close_idx]
            before = text[max(0, m.start() - 40):m.start()]
            after = text[close_idx + 1:close_idx + 40]
            if not IDENT.match(arg.strip()):
                skipped.append(("complex arg", f"{variant}({arg.strip()[:30]})"))
                continue
            if is_pattern_position(before, after):
                skipped.append(("pattern", f"{variant}({arg.strip()})"))
                continue
            edits.append((open_idx + 1, close_idx, f"Arc::new({arg.strip()})"))

    out = []
    last = 0
    for start, end, rep in sorted(edits):
        out.append(text[last:start])
        out.append(rep)
        last = end
    out.append(text[last:])
    return "".join(out), edits, skipped


def main():
    apply = "--dry-run" not in sys.argv
    text = open(SRC).read()
    new_text, edits, skipped = rewrite(text, apply)

    print(f"{'APPLIED' if apply else 'DRY RUN'}: {len(edits)} constructions wrapped")
    from collections import Counter
    for kind, count in Counter(k for k, _ in skipped).most_common():
        print(f"  skipped ({kind}): {count}")
    print("  --- skipped samples (fix these by hand) ---")
    for kind, sample in skipped[:25]:
        print(f"    [{kind}] {sample}")

    if apply and len(edits):
        open(SRC, "w").write(new_text)
        print("written")


if __name__ == "__main__":
    main()
