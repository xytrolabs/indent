from __future__ import annotations

import ast
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from .ast_nodes import (
    ArgItem,
    Assign,
    BareExpr,
    Call,
    DefFun,
    DefVar,
    Give,
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
from .parser import ParseError, parse_source


class RuntimeErrorIndent(Exception):
    pass


class _LoopStop(Exception):
    pass


class _LoopNext(Exception):
    pass


class _LoopReset(Exception):
    pass


class _ReturnSignal(Exception):
    def __init__(self, value: Any):
        self.value = value


@dataclass
class IndentFunction:
    name: str
    body: List[Node]
    closure: "Scope"


@dataclass
class ModuleRef:
    name: str
    runtime: "IndentRuntime"


@dataclass
class Scope:
    parent: Optional["Scope"] = None
    vars: Dict[str, Any] = field(default_factory=dict)
    funcs: Dict[str, IndentFunction] = field(default_factory=dict)
    modules: Dict[str, ModuleRef] = field(default_factory=dict)

    def define_var(self, name: str, value: Any) -> None:
        self.vars[name] = value

    def set_var(self, name: str, value: Any) -> None:
        if name in self.vars:
            self.vars[name] = value
            return
        if self.parent is not None:
            self.parent.set_var(name, value)
            return
        self.vars[name] = value

    def get_var(self, name: str) -> Any:
        if name in self.vars:
            return self.vars[name]
        if self.parent is not None:
            return self.parent.get_var(name)
        raise RuntimeErrorIndent(f"Undefined variable '{name}'")

    def define_func(self, name: str, fn: IndentFunction) -> None:
        self.funcs[name] = fn

    def get_func(self, name: str) -> IndentFunction:
        if name in self.funcs:
            return self.funcs[name]
        if self.parent is not None:
            return self.parent.get_func(name)
        raise RuntimeErrorIndent(f"Undefined function '{name}'")

    def define_module(self, name: str, module: ModuleRef) -> None:
        self.modules[name] = module

    def get_module(self, name: str) -> ModuleRef:
        if name in self.modules:
            return self.modules[name]
        if self.parent is not None:
            return self.parent.get_module(name)
        raise RuntimeErrorIndent(f"Undefined module '{name}'")

    def has_name(self, name: str) -> bool:
        if name in self.vars or name in self.funcs or name in self.modules:
            return True
        if self.parent is not None:
            return self.parent.has_name(name)
        return False


class SafeExprEvaluator(ast.NodeVisitor):
    def __init__(self, scope: Scope):
        self.scope = scope

    def evaluate(self, expr: str) -> Any:
        expr = expr.strip()
        if not expr:
            return None
        tree = ast.parse(expr, mode="eval")
        return self.visit(tree.body)

    def visit_Constant(self, node: ast.Constant) -> Any:
        return node.value

    def visit_Name(self, node: ast.Name) -> Any:
        constants = {
            "TRUE": True,
            "FALSE": False,
            "YES": True,
            "NO": False,
            "empty": None,
        }
        if node.id in constants:
            return constants[node.id]

        if self.scope.has_name(node.id):
            if node.id in self.scope.modules:
                return self.scope.modules[node.id]
            try:
                return self.scope.get_var(node.id)
            except RuntimeErrorIndent:
                return self.scope.get_func(node.id)

        raise RuntimeErrorIndent(f"Unknown identifier in expression: '{node.id}'")

    def visit_List(self, node: ast.List) -> Any:
        return [self.visit(elt) for elt in node.elts]

    def visit_Tuple(self, node: ast.Tuple) -> Any:
        return tuple(self.visit(elt) for elt in node.elts)

    def visit_Set(self, node: ast.Set) -> Any:
        return {self.visit(elt) for elt in node.elts}

    def visit_Dict(self, node: ast.Dict) -> Any:
        return {self.visit(k): self.visit(v) for k, v in zip(node.keys, node.values)}

    def visit_UnaryOp(self, node: ast.UnaryOp) -> Any:
        operand = self.visit(node.operand)
        if isinstance(node.op, ast.USub):
            return -operand
        if isinstance(node.op, ast.UAdd):
            return +operand
        if isinstance(node.op, ast.Not):
            return not operand
        raise RuntimeErrorIndent("Unsupported unary operator")

    def visit_BinOp(self, node: ast.BinOp) -> Any:
        left = self.visit(node.left)
        right = self.visit(node.right)

        if isinstance(node.op, ast.Add):
            return left + right
        if isinstance(node.op, ast.Sub):
            return left - right
        if isinstance(node.op, ast.Mult):
            return left * right
        if isinstance(node.op, ast.Div):
            return left / right
        if isinstance(node.op, ast.FloorDiv):
            return left // right
        if isinstance(node.op, ast.Mod):
            return left % right
        if isinstance(node.op, ast.Pow):
            return left**right

        raise RuntimeErrorIndent("Unsupported binary operator")

    def visit_BoolOp(self, node: ast.BoolOp) -> Any:
        if isinstance(node.op, ast.And):
            for value in node.values:
                result = self.visit(value)
                if not result:
                    return result
            return result

        if isinstance(node.op, ast.Or):
            for value in node.values:
                result = self.visit(value)
                if result:
                    return result
            return result

        raise RuntimeErrorIndent("Unsupported boolean operation")

    def visit_Compare(self, node: ast.Compare) -> Any:
        left = self.visit(node.left)
        for op, comparator in zip(node.ops, node.comparators):
            right = self.visit(comparator)

            ok = False
            if isinstance(op, ast.Eq):
                ok = left == right
            elif isinstance(op, ast.NotEq):
                ok = left != right
            elif isinstance(op, ast.Gt):
                ok = left > right
            elif isinstance(op, ast.GtE):
                ok = left >= right
            elif isinstance(op, ast.Lt):
                ok = left < right
            elif isinstance(op, ast.LtE):
                ok = left <= right
            elif isinstance(op, ast.In):
                ok = left in right
            elif isinstance(op, ast.NotIn):
                ok = left not in right
            else:
                raise RuntimeErrorIndent("Unsupported comparison operator")

            if not ok:
                return False
            left = right
        return True

    def visit_Subscript(self, node: ast.Subscript) -> Any:
        value = self.visit(node.value)
        index = self.visit(node.slice)
        return value[index]

    def visit_Attribute(self, node: ast.Attribute) -> Any:
        obj = self.visit(node.value)
        if isinstance(obj, ModuleRef):
            # Support access in expressions such as module.var.
            if node.attr in obj.runtime.global_scope.vars:
                return obj.runtime.global_scope.vars[node.attr]
            if node.attr in obj.runtime.global_scope.funcs:
                return obj.runtime.global_scope.funcs[node.attr]
            raise RuntimeErrorIndent(
                f"Module '{obj.name}' has no attribute '{node.attr}'"
            )
        # Dict dot-notation: person.name → person["name"]
        if isinstance(obj, dict):
            if node.attr in obj:
                return obj[node.attr]
            raise RuntimeErrorIndent(
                f"Dict has no key '{node.attr}' (available: {list(obj.keys())})"
            )
        raise RuntimeErrorIndent(
            f"Attribute access is only allowed on modules and dictionaries, got {type(obj).__name__}"
        )

    def generic_visit(self, node: ast.AST) -> Any:
        raise RuntimeErrorIndent(f"Unsupported expression syntax: {type(node).__name__}")


class IndentRuntime:
    def __init__(self, module_path: Optional[Path] = None):
        self.module_path = module_path
        self.global_scope = Scope()
        self._module_cache: Dict[Path, IndentRuntime] = {}

    def run_source(self, source: str) -> None:
        program = parse_source(source)
        self._exec_program(program, self.global_scope)

    def run_file(self, file_path: str | Path) -> None:
        path = Path(file_path).resolve()
        self.module_path = path.parent
        source = path.read_text(encoding="utf-8")
        self.run_source(source)

    def _eval(self, expr: str, scope: Scope) -> Any:
        evaluator = SafeExprEvaluator(scope)
        try:
            return evaluator.evaluate(expr)
        except SyntaxError as exc:
            raise RuntimeErrorIndent(f"Invalid expression '{expr}': {exc}") from exc

    def _coerce_type(self, declared_type: str, value: Any, line: int, name: str) -> Any:
        t = declared_type.lower()
        if t == "string":
            if isinstance(value, str):
                return value
            raise RuntimeErrorIndent(
                f"Line {line}: variable '{name}' expects string, got {type(value).__name__}"
            )
        if t == "int":
            if isinstance(value, bool):
                raise RuntimeErrorIndent(
                    f"Line {line}: variable '{name}' expects int, got boolean"
                )
            if isinstance(value, int):
                return value
            raise RuntimeErrorIndent(
                f"Line {line}: variable '{name}' expects int, got {type(value).__name__}"
            )
        if t == "float":
            if isinstance(value, (int, float)) and not isinstance(value, bool):
                return float(value)
            raise RuntimeErrorIndent(
                f"Line {line}: variable '{name}' expects float, got {type(value).__name__}"
            )
        if t == "boolean":
            if isinstance(value, bool):
                return value
            raise RuntimeErrorIndent(
                f"Line {line}: variable '{name}' expects boolean, got {type(value).__name__}"
            )
        if t == "list":
            if isinstance(value, list):
                return value
            raise RuntimeErrorIndent(
                f"Line {line}: variable '{name}' expects list, got {type(value).__name__}"
            )
        if t == "dict":
            if isinstance(value, dict):
                return value
            raise RuntimeErrorIndent(
                f"Line {line}: variable '{name}' expects dict, got {type(value).__name__}"
            )
        return value

    def _exec_program(self, program: Program, scope: Scope) -> None:
        for stmt in program.body:
            self._exec_stmt(stmt, scope)

    def _exec_stmt(self, stmt: Node, scope: Scope) -> Any:
        if isinstance(stmt, Say):
            print(self._eval(stmt.expr, scope))
            return None

        if isinstance(stmt, DefVar):
            value = self._eval(stmt.value_expr, scope)
            value = self._coerce_type(stmt.declared_type, value, stmt.line, stmt.name)
            scope.define_var(stmt.name, value)
            return None

        if isinstance(stmt, Assign):
            value = self._eval(stmt.value_expr, scope)
            scope.set_var(stmt.name, value)
            return None

        if isinstance(stmt, DefFun):
            scope.define_func(stmt.name, IndentFunction(stmt.name, stmt.body, scope))
            return None

        if isinstance(stmt, Give):
            raise _ReturnSignal(self._eval(stmt.expr, scope))

        if isinstance(stmt, IfChain):
            for branch in stmt.branches:
                if branch.condition is None or bool(self._eval(branch.condition, scope)):
                    for inner in branch.body:
                        self._exec_stmt(inner, scope)
                    break
            return None

        if isinstance(stmt, Repeat):
            self._exec_repeat(stmt, scope)
            return None

        if isinstance(stmt, Stop):
            raise _LoopStop()

        if isinstance(stmt, Next):
            raise _LoopNext()

        if isinstance(stmt, Reset):
            raise _LoopReset()

        if isinstance(stmt, Import):
            self._exec_import(stmt, scope)
            return None

        if isinstance(stmt, Call):
            self._exec_call(stmt, scope)
            return None

        if isinstance(stmt, BareExpr):
            self._eval(stmt.expr, scope)
            return None

        raise RuntimeErrorIndent(f"Unhandled statement type: {type(stmt).__name__}")

    def _execute_block(self, body: List[Node], scope: Scope) -> None:
        for stmt in body:
            self._exec_stmt(stmt, scope)

    def _exec_repeat(self, stmt: Repeat, scope: Scope) -> None:
        max_iterations = 100_000

        def run_body(loop_scope: Scope) -> Tuple[bool, bool]:
            try:
                self._execute_block(stmt.body, loop_scope)
                return False, False
            except _LoopNext:
                return False, False
            except _LoopReset:
                return True, False
            except _LoopStop:
                return False, True

        if stmt.mode == "infinite":
            reps = 0
            while reps < max_iterations:
                loop_scope = Scope(parent=scope)
                loop_scope.define_var("Reps", reps)
                should_reset, should_stop = run_body(loop_scope)
                if should_stop:
                    break
                if should_reset:
                    reps = 0
                    continue
                reps += 1
            else:
                raise RuntimeErrorIndent("Repeat loop exceeded safety limit")
            return

        if stmt.mode == "count":
            total = int(self._eval(stmt.count_expr or "0", scope))
            reps = 0
            while reps < total:
                loop_scope = Scope(parent=scope)
                loop_scope.define_var("Reps", reps)
                should_reset, should_stop = run_body(loop_scope)
                if should_stop:
                    break
                if should_reset:
                    reps = 0
                    continue
                reps += 1
            return

        if stmt.mode == "for_each":
            iterable = self._eval(stmt.iterable_expr or "[]", scope)
            reps = 0
            for item in iterable:
                loop_scope = Scope(parent=scope)
                loop_scope.define_var("Reps", reps)
                loop_scope.define_var("Item", item)
                should_reset, should_stop = run_body(loop_scope)
                if should_stop:
                    break
                if should_reset:
                    reps = 0
                    continue
                reps += 1
            return

        if stmt.mode == "for_in":
            iterable = self._eval(stmt.iterable_expr or "[]", scope)
            reps = 0
            for item in iterable:
                loop_scope = Scope(parent=scope)
                loop_scope.define_var("Reps", reps)
                loop_scope.define_var(stmt.item_name or "Item", item)
                should_reset, should_stop = run_body(loop_scope)
                if should_stop:
                    break
                if should_reset:
                    reps = 0
                    continue
                reps += 1
            return

        if stmt.mode == "until":
            reps = 0
            while reps < max_iterations:
                if bool(self._eval(stmt.condition_expr or "FALSE", scope)):
                    break
                loop_scope = Scope(parent=scope)
                loop_scope.define_var("Reps", reps)
                should_reset, should_stop = run_body(loop_scope)
                if should_stop:
                    break
                if should_reset:
                    reps = 0
                    continue
                reps += 1
            else:
                raise RuntimeErrorIndent("Repeat until loop exceeded safety limit")
            return

        raise RuntimeErrorIndent(f"Unknown repeat mode: {stmt.mode}")

    def _resolve_module_file(self, module_name: str) -> Path:
        if self.module_path is None:
            base = Path.cwd()
        else:
            base = self.module_path
        target = (base / f"{module_name}.ind").resolve()
        if not target.exists():
            raise RuntimeErrorIndent(f"Cannot import module '{module_name}': file not found")
        return target

    def _load_module_runtime(self, module_name: str) -> ModuleRef:
        mod_file = self._resolve_module_file(module_name)
        if mod_file in self._module_cache:
            return ModuleRef(module_name, self._module_cache[mod_file])

        mod_runtime = IndentRuntime(module_path=mod_file.parent)
        mod_runtime._module_cache = self._module_cache
        mod_runtime.run_file(mod_file)
        self._module_cache[mod_file] = mod_runtime
        return ModuleRef(module_name, mod_runtime)

    def _exec_import(self, stmt: Import, scope: Scope) -> None:
        module_ref = self._load_module_runtime(stmt.module_name)

        if stmt.symbol_name is None:
            bind_name = stmt.alias or stmt.module_name
            scope.define_module(bind_name, module_ref)
            scope.define_var(bind_name, module_ref)
            return

        export_name = stmt.symbol_name
        bind_name = stmt.alias or export_name

        mod_scope = module_ref.runtime.global_scope
        if export_name in mod_scope.funcs:
            scope.define_func(bind_name, mod_scope.funcs[export_name])
            return
        if export_name in mod_scope.vars:
            scope.define_var(bind_name, mod_scope.vars[export_name])
            return

        raise RuntimeErrorIndent(
            f"Module '{stmt.module_name}' does not export '{export_name}'"
        )

    def _resolve_callable(self, callee: str, scope: Scope) -> IndentFunction:
        if "." in callee:
            left, right = callee.split(".", 1)
            module = scope.get_module(left)
            mod_scope = module.runtime.global_scope
            if right in mod_scope.funcs:
                return mod_scope.funcs[right]
            raise RuntimeErrorIndent(
                f"Module '{left}' has no function '{right}'"
            )

        return scope.get_func(callee)

    def _exec_call(self, stmt: Call, scope: Scope) -> Any:
        fn = self._resolve_callable(stmt.callee, scope)

        positional: List[Any] = []
        named: Dict[str, Any] = {}

        arg_scope = Scope(parent=scope)
        for item in stmt.args:
            if item.kind == "statement" and item.statement is not None:
                self._exec_stmt(item.statement, arg_scope)
                continue
            if item.kind == "positional" and item.expr is not None:
                positional.append(self._eval(item.expr, arg_scope))
                continue
            if item.kind == "named" and item.name is not None and item.expr is not None:
                named[item.name] = self._eval(item.expr, arg_scope)
                continue

        return self._invoke_function(fn, positional, named)

    def _invoke_function(
        self,
        fn: IndentFunction,
        positional: List[Any],
        named: Dict[str, Any],
    ) -> Any:
        call_scope = Scope(parent=fn.closure)

        for idx, value in enumerate(positional):
            if idx == 0:
                key = "argument"
            elif idx == 1:
                key = "argument2"
            else:
                key = f"argument{idx + 1}"
            call_scope.define_var(key, value)

        for key, value in named.items():
            call_scope.define_var(key, value)

        try:
            self._execute_block(fn.body, call_scope)
        except _ReturnSignal as ret:
            return ret.value
        return None


def run_file(file_path: str | Path) -> None:
    runtime = IndentRuntime()
    try:
        runtime.run_file(file_path)
    except ParseError as exc:
        raise RuntimeErrorIndent(f"Parse error: {exc}") from exc
