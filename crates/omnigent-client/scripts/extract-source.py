"""Extract what tests/contract.rs checks from Omnigent's source, where its OpenAPI
document has nothing: the fields of the request models (pydantic classes in
omnigent/server/schemas.py, with the fields of their in-file bases) and the routes hidden
from the document (`include_in_schema=False`) in the given route files.

The source is parsed (`ast`), never imported or run.

Usage: python3 -I extract-source.py <out.json> <schemas.py> <routes.py>...
Each routes file is named by its path in the upstream repository after a `=`:
  extract-source.py out.json schemas.py routes_events.py=omnigent/server/routes/sessions/routes_events.py
"""

import ast
import json
import sys

METHODS = {"get", "post", "put", "patch", "delete"}


def models(tree):
    classes = {node.name: node for node in tree.body if isinstance(node, ast.ClassDef)}

    def fields(name, seen=()):
        node = classes.get(name)
        if node is None or name in seen:
            return []
        out = []
        for base in node.bases:
            if isinstance(base, ast.Name):
                out += fields(base.id, seen + (name,))
        for statement in node.body:
            if isinstance(statement, ast.AnnAssign) and isinstance(statement.target, ast.Name):
                field = statement.target.id
                if not field.startswith("_") and field != "model_config" and field not in out:
                    out.append(field)
        return out

    return {name: fields(name) for name in sorted(classes)}


def hidden_routes(tree, file):
    routes = []
    for node in ast.walk(tree):
        if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            continue
        for decorator in node.decorator_list:
            if not (
                isinstance(decorator, ast.Call)
                and isinstance(decorator.func, ast.Attribute)
                and decorator.func.attr in METHODS
                and decorator.args
                and isinstance(decorator.args[0], ast.Constant)
            ):
                continue
            hidden = any(
                k.arg == "include_in_schema"
                and isinstance(k.value, ast.Constant)
                and k.value.value is False
                for k in decorator.keywords
            )
            if hidden:
                # The routers are mounted under /v1 (omnigent/server/app.py).
                routes.append([decorator.func.attr, "/v1" + decorator.args[0].value, file])
    return sorted(routes)


def main():
    out, schemas, *route_files = sys.argv[1:]
    with open(schemas, encoding="utf-8") as f:
        result = {"models": models(ast.parse(f.read()))}
    result["hidden_routes"] = []
    for arg in route_files:
        path, _, name = arg.partition("=")
        with open(path, encoding="utf-8") as f:
            result["hidden_routes"] += hidden_routes(ast.parse(f.read()), name or path)
    result["hidden_routes"].sort()
    with open(out, "w", encoding="utf-8") as f:
        json.dump(result, f, indent=1, sort_keys=True)
        f.write("\n")


if __name__ == "__main__":
    main()
