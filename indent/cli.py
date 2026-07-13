from __future__ import annotations

import argparse
import sys

from .runtime import RuntimeErrorIndent, run_file


def main() -> int:
    parser = argparse.ArgumentParser(description="Run Indent (.ind) files")
    parser.add_argument("file", help="Path to the .ind file")
    args = parser.parse_args()

    try:
        run_file(args.file)
    except RuntimeErrorIndent as exc:
        print(f"Indent error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
