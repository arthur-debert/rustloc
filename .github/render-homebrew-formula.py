#!/usr/bin/env python3
"""Render a Homebrew formula from a template.

Reads values from environment variables, substitutes {{PLACEHOLDER}} tokens
in the template, and writes the result. String-typed values (DESC, LICENSE,
HOMEPAGE) are escaped so they survive embedding in a Ruby double-quoted
string, even when they contain quotes, backslashes, or interpolation
metacharacters.

Called from .github/workflows/release.yml. Not a general-purpose tool.

Env-var keys are advisory: any RUBY_STRING_KEYS value present in the env
is escaped and substituted; any PASSTHROUGH_KEYS value present is
substituted verbatim. Missing env vars are tolerated — the
find_unsubstituted post-check fails the run if any {{PLACEHOLDER}} survives
in the rendered output, which is what actually matters. This lets a single
script serve projects whose templates use different subsets of placeholders
(e.g. arm-only mac vs arm+intel).
"""
from __future__ import annotations

import os
import re
import sys
from pathlib import Path


# Values that, when present in the env, get embedded inside a Ruby `"..."`
# literal and must therefore be escaped.
RUBY_STRING_KEYS = {"DESC", "LICENSE", "HOMEPAGE"}

# Values that, when present in the env, are passed through verbatim. URLs,
# SHAs, class names, and bin names are all ASCII-safe by construction —
# escaping them would only add noise to the generated formula.
PASSTHROUGH_KEYS = [
    "CLASS_NAME",
    "VERSION",
    "BIN_NAME",
    "URL_AARCH64_APPLE_DARWIN",
    "URL_X86_64_APPLE_DARWIN",
    "URL_X86_64_LINUX_GNU",
    "URL_AARCH64_LINUX_GNU",
    "SHA_AARCH64_APPLE_DARWIN",
    "SHA_X86_64_APPLE_DARWIN",
    "SHA_X86_64_LINUX_GNU",
    "SHA_AARCH64_LINUX_GNU",
]


def ruby_double_quoted_escape(s: str) -> str:
    """Escape `s` for embedding inside a Ruby `"..."` string literal.

    Covers: backslash, double quote, `#{` interpolation, and control chars.
    """
    out = []
    for ch in s:
        if ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        elif ch == "#":
            # Defuse `#{...}` interpolation by escaping every `#`.
            # Harmless in contexts where `#` isn't followed by `{`.
            out.append("\\#")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\t":
            out.append("\\t")
        elif ch == "\r":
            out.append("\\r")
        elif ord(ch) < 0x20:
            out.append(f"\\x{ord(ch):02x}")
        else:
            out.append(ch)
    return "".join(out)


def build_replacements() -> dict[str, str]:
    replacements: dict[str, str] = {}
    for key in PASSTHROUGH_KEYS:
        value = os.environ.get(key)
        if value is not None:
            replacements[key] = value
    for key in RUBY_STRING_KEYS:
        raw = os.environ.get(key)
        if raw is not None:
            replacements[key] = ruby_double_quoted_escape(raw)
    return replacements


def render(template: str, replacements: dict[str, str]) -> str:
    for key, value in replacements.items():
        template = template.replace(f"{{{{{key}}}}}", value)
    return template


def find_unsubstituted(rendered: str) -> list[str]:
    """Find {{PLACEHOLDER}} tokens that survived substitution, excluding
    those inside comment lines (the template's header comment documents
    the placeholder syntax using literal `{{NAME}}` tokens)."""
    leftover: list[str] = []
    for line in rendered.splitlines():
        if line.lstrip().startswith("#"):
            continue
        leftover.extend(re.findall(r"\{\{[A-Z_]+\}\}", line))
    return leftover


def main() -> int:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} <template> <output>", file=sys.stderr)
        return 2
    template_path = Path(sys.argv[1])
    output_path = Path(sys.argv[2])

    replacements = build_replacements()
    rendered = render(template_path.read_text(encoding="utf-8"), replacements)

    leftover = find_unsubstituted(rendered)
    if leftover:
        print(
            f"::error::Unsubstituted placeholders in generated formula: {leftover}",
            file=sys.stderr,
        )
        return 1

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(rendered, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
