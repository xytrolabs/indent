from __future__ import annotations

import re
from dataclasses import dataclass
from typing import List, Optional

from .ast_nodes import (
    ArgItem,
    Assign,
    BareExpr,
    Call,
    DefFun,
    DefVar,
    Give,
    IfBranch,
    IfChain,
    Import,
    Next,
    Node,
    Program,
    Repeat,
    Reset,
    Say,
    Stop,
)


@dataclass
class SourceLine:
    line_no: int
    indent: int
    text: str


class ParseError(Exception):
    pass


def _strip_inline_comment(line: str) -> str:
    in_string = False
    escaped = False
    quote = ""
    i = 0
    while i < len(line):
        c = line[i]
        if escaped:
            escaped = False
        elif c == "\\":
            escaped = True
        elif in_string:
            if c == quote:
                in_string = False
                quote = ""
        else:
            if c in ('"', "'"):
                in_string = True
                quote = c
            elif c == "#" and i + 1 < len(line) and line[i + 1] == "!":
                return line[:i]
        i += 1
    return line


def preprocess(source: str) -> List[SourceLine]:
    raw_lines = source.splitlines()
    out: List[SourceLine] = []
    in_multiline_comment = False

    for idx, raw in enumerate(raw_lines, start=1):
        if in_multiline_comment:
            if "#!*" in raw:
                in_multiline_comment = False
            continue

        if "#!*" in raw:
            in_multiline_comment = True
            continue

        line = _strip_inline_comment(raw).rstrip("\n\r")
        if not line.strip():
            continue

        expanded = line.replace("\t", "    ")
        indent = len(expanded) - len(expanded.lstrip(" "))
        text = expanded.strip()
        out.append(SourceLine(line_no=idx, indent=indent, text=text))

    return out


class Parser:
    def __init__(self, lines: List[SourceLine]):
        self.lines = lines
        self.i = 0

    def parse(self) -> Program:
        body = self._parse_block(expected_indent=0)
        return Program(line=1, body=body)

    def _peek(self) -> Optional[SourceLine]:
        if self.i >= len(self.lines):
            return None
        return self.lines[self.i]

    def _consume(self) -> SourceLine:
        line = self._peek()
        if line is None:
            raise ParseError("Unexpected end of file")
        self.i += 1
        return line

    def _parse_block(self, expected_indent: int) -> List[Node]:
        stmts: List[Node] = []
        while True:
            line = self._peek()
            if line is None:
                break
            if line.indent < expected_indent:
                break
            if line.indent > expected_indent:
                raise ParseError(
                    f"Unexpected indentation at line {line.line_no}: '{line.text}'"
                )

            stmt = self._parse_statement(expected_indent)
            stmts.append(stmt)

        return stmts

    def _parse_statement(self, expected_indent: int) -> Node:
        line = self._consume()
        text = line.text

        if text.startswith("say:"):
            return Say(line=line.line_no, expr=text[len("say:") :].strip())

        if text.startswith("Give:"):
            return Give(line=line.line_no, expr=text[len("Give:") :].strip())

        if text.startswith("def.var:"):
            return self._parse_def_var(line)

        if text.startswith("def.fun:"):
            name = text[len("def.fun:") :].strip()
            if not name:
                raise ParseError(f"Function name missing at line {line.line_no}")
            body = self._parse_nested_block(line)
            return DefFun(line=line.line_no, name=name, body=body)

        if text.startswith("if ") and text.endswith(":"):
            return self._parse_if_chain(line)

        if text.startswith("Repeat"):
            return self._parse_repeat(line)

        if text == "STOP":
            return Stop(line=line.line_no)

        if text == "NEXT":
            return Next(line=line.line_no)

        if text == "RESET":
            return Reset(line=line.line_no)

        if text.startswith("Get:"):
            return self._parse_import(line)

        if text.endswith(";"):
            return self._parse_call_with_args(line)

        if self._looks_like_callee(text):
            return Call(line=line.line_no, callee=text, args=[])

        assign_match = re.match(r"^([A-Za-z_][A-Za-z0-9_]*)\s+is\s+(.+)$", text)
        if assign_match:
            return Assign(
                line=line.line_no,
                name=assign_match.group(1),
                value_expr=assign_match.group(2).strip(),
            )

        return BareExpr(line=line.line_no, expr=text)

    def _parse_nested_block(self, header: SourceLine) -> List[Node]:
        nxt = self._peek()
        if nxt is None or nxt.indent <= header.indent:
            return []
        return self._parse_block(nxt.indent)

    def _parse_def_var(self, header: SourceLine) -> DefVar:
        name = header.text[len("def.var:") :].strip()
        if not name:
            raise ParseError(f"Variable name missing at line {header.line_no}")

        t_line = self._peek()
        if t_line is None or t_line.indent <= header.indent:
            raise ParseError(
                f"Expected type line for variable '{name}' at line {header.line_no}"
            )
        self._consume()

        v_line = self._peek()
        if v_line is None or v_line.indent != t_line.indent:
            raise ParseError(
                f"Expected value line for variable '{name}' after line {t_line.line_no}"
            )
        self._consume()

        return DefVar(
            line=header.line_no,
            name=name,
            declared_type=t_line.text.strip(),
            value_expr=v_line.text.strip(),
        )

    def _parse_if_chain(self, first: SourceLine) -> IfChain:
        branches: List[IfBranch] = []
        cond = first.text[len("if") : -1].strip()
        first_body = self._parse_nested_block(first)
        branches.append(IfBranch(condition=cond, body=first_body))

        while True:
            nxt = self._peek()
            if nxt is None or nxt.indent != first.indent:
                break

            if nxt.text.startswith("or ") and nxt.text.endswith(":"):
                self._consume()
                elif_cond = nxt.text[len("or") : -1].strip()
                elif_body = self._parse_nested_block(nxt)
                branches.append(IfBranch(condition=elif_cond, body=elif_body))
                continue

            if nxt.text == "otherwise:":
                self._consume()
                else_body = self._parse_nested_block(nxt)
                branches.append(IfBranch(condition=None, body=else_body))
                break

            break

        return IfChain(line=first.line_no, branches=branches)

    def _parse_repeat(self, header: SourceLine) -> Repeat:
        text = header.text
        body = self._parse_nested_block(header)

        if text == "Repeat:":
            return Repeat(line=header.line_no, mode="infinite", body=body)

        m_count = re.match(r"^Repeat\s+(.+):$", text)
        if m_count:
            chunk = m_count.group(1).strip()

            m_for_in = re.match(r"^for\s+([A-Za-z_][A-Za-z0-9_]*)\s+in\s+(.+)$", chunk)
            if m_for_in:
                return Repeat(
                    line=header.line_no,
                    mode="for_in",
                    body=body,
                    item_name=m_for_in.group(1),
                    iterable_expr=m_for_in.group(2).strip(),
                )

            m_for_each = re.match(r"^for\s+(.+)$", chunk)
            if m_for_each:
                return Repeat(
                    line=header.line_no,
                    mode="for_each",
                    body=body,
                    iterable_expr=m_for_each.group(1).strip(),
                )

            m_until = re.match(r"^until\s+(.+)$", chunk)
            if m_until:
                return Repeat(
                    line=header.line_no,
                    mode="until",
                    body=body,
                    condition_expr=m_until.group(1).strip(),
                )

            return Repeat(
                line=header.line_no,
                mode="count",
                body=body,
                count_expr=chunk,
            )

        raise ParseError(f"Invalid Repeat syntax at line {header.line_no}")

    def _parse_import(self, line: SourceLine) -> Import:
        text = line.text

        m1 = re.match(r"^Get:\s*([A-Za-z_][A-Za-z0-9_]*)$", text)
        if m1:
            return Import(line=line.line_no, module_name=m1.group(1))

        m2 = re.match(
            r"^Get:\s*([A-Za-z_][A-Za-z0-9_]*)\s+From:\s*([A-Za-z_][A-Za-z0-9_]*)(?:\s+As:\s*([A-Za-z_][A-Za-z0-9_]*))?$",
            text,
        )
        if m2:
            return Import(
                line=line.line_no,
                module_name=m2.group(2),
                symbol_name=m2.group(1),
                alias=m2.group(3),
            )

        m3 = re.match(
            r"^Get:\s*([A-Za-z_][A-Za-z0-9_]*)\s+As:\s*([A-Za-z_][A-Za-z0-9_]*)$",
            text,
        )
        if m3:
            return Import(
                line=line.line_no,
                module_name=m3.group(1),
                alias=m3.group(2),
            )

        raise ParseError(f"Invalid import syntax at line {line.line_no}: '{text}'")

    def _parse_call_with_args(self, line: SourceLine) -> Call:
        callee = line.text[:-1].strip()
        if not self._looks_like_callee(callee):
            raise ParseError(f"Invalid function call at line {line.line_no}: '{line.text}'")

        child = self._peek()
        args: List[ArgItem] = []
        if child is None or child.indent <= line.indent:
            return Call(line=line.line_no, callee=callee, args=args)

        child_indent = child.indent
        while True:
            nxt = self._peek()
            if nxt is None or nxt.indent < child_indent:
                break
            if nxt.indent > child_indent:
                raise ParseError(
                    f"Unexpected indentation in call arguments at line {nxt.line_no}"
                )

            if nxt.text.startswith("def.var:"):
                stmt = self._parse_statement(child_indent)
                args.append(ArgItem(kind="statement", statement=stmt))
                continue

            assign_match = re.match(
                r"^([A-Za-z_][A-Za-z0-9_]*)\s+is\s+(.+)$", nxt.text
            )
            if assign_match:
                self._consume()
                args.append(
                    ArgItem(
                        kind="named",
                        name=assign_match.group(1),
                        expr=assign_match.group(2).strip(),
                    )
                )
                continue

            self._consume()
            args.append(ArgItem(kind="positional", expr=nxt.text))

        return Call(line=line.line_no, callee=callee, args=args)

    @staticmethod
    def _looks_like_callee(text: str) -> bool:
        return bool(re.match(r"^[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*$", text))


def parse_source(source: str) -> Program:
    return Parser(preprocess(source)).parse()
