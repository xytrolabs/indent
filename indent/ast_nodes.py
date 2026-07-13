from __future__ import annotations

from dataclasses import dataclass
from typing import List, Optional


@dataclass
class Node:
    line: int


@dataclass
class Program(Node):
    body: List[Node]


@dataclass
class Say(Node):
    expr: str


@dataclass
class DefVar(Node):
    name: str
    declared_type: str
    value_expr: str


@dataclass
class Assign(Node):
    name: str
    value_expr: str


@dataclass
class DefFun(Node):
    name: str
    body: List[Node]


@dataclass
class Give(Node):
    expr: str


@dataclass
class IfBranch:
    condition: Optional[str]
    body: List[Node]


@dataclass
class IfChain(Node):
    branches: List[IfBranch]


@dataclass
class Repeat(Node):
    mode: str  # infinite | count | for_each | for_in | until
    body: List[Node]
    count_expr: Optional[str] = None
    iterable_expr: Optional[str] = None
    item_name: Optional[str] = None
    condition_expr: Optional[str] = None


@dataclass
class Stop(Node):
    pass


@dataclass
class Next(Node):
    pass


@dataclass
class Reset(Node):
    pass


@dataclass
class Import(Node):
    module_name: str
    symbol_name: Optional[str] = None
    alias: Optional[str] = None


@dataclass
class ArgItem:
    kind: str  # positional | named | statement
    expr: Optional[str] = None
    name: Optional[str] = None
    statement: Optional[Node] = None


@dataclass
class Call(Node):
    callee: str
    args: List[ArgItem]


@dataclass
class BareExpr(Node):
    expr: str
