"""Embed R in the isolated Python worker; keep R objects in R during timing."""

from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import Any, cast

from benchmarks.harness import BackendUnavailable


class CanivtBackend:
    def __init__(self, operation: str, path: Path, library: Path | None) -> None:
        if operation not in ("metadata", "arrow"):
            raise ValueError(f"Unknown operation: {operation}")
        if os.name == "nt":
            # Windows uses ABI mode; C.UTF-8 is not a valid Windows R locale.
            os.environ.setdefault("RPY2_CFFI_MODE", "ABI")
            os.environ.setdefault("LC_ALL", "C")
        try:
            from rpy2.rinterface_lib import callbacks, embedded

            cast(Any, embedded).set_initoptions(("rpy2", "--quiet", "--vanilla"))

            from rpy2 import robjects
        except (ImportError, OSError, RuntimeError, ValueError) as error:
            raise BackendUnavailable(
                "canivt requires rpy2 and a working R installation; "
                "run uv sync --group benchmark and set R_HOME if needed. "
                f"{error}"
            ) from error

        # R output must not corrupt the worker's stdout JSON protocol.
        def console_write(s: str) -> None:
            sys.stderr.write(s)

        cast(Any, callbacks).consolewrite_print = console_write
        # R expressions are dynamically typed; rpy2 annotates their results as object.
        self.r: Any = robjects.r
        if library is not None:
            self.r(
                r"""function(root) {
                    version <- paste0("R-", R.version$major, ".",
                        strsplit(R.version$minor, ".", fixed = TRUE)[[1]][1])
                    .libPaths(c(file.path(root, version), root, .libPaths()))
                }"""
            )(str(library.resolve()))
        for package in ("canivt", "arrow") if operation == "arrow" else ("canivt",):
            if not self.r["requireNamespace"](package, quietly=True)[0]:
                raise BackendUnavailable(f"R package {package} is not installed")
        package_version = self.r(
            "function(package) as.character(utils::packageVersion(package))"
        )
        self.version = str(package_version("canivt")[0])
        self.runtime = str(self.r("as.character(getRversion())")[0])
        self.arrow_version = (
            str(package_version("arrow")[0]) if operation == "arrow" else None
        )
        self.operation = operation
        self.path = str(path)
        self._run = self.r(
            "function(source) canivt::ivt_metadata(source)"
            if operation == "metadata"
            else """function(source) {
                ivt <- canivt::read_ivt(source, missing = TRUE, complete = TRUE)
                arrow::Table$create(canivt::ivt_tidy(ivt, missing = TRUE))
            }"""
        )
        self._describe = self.r(
            """function(result) {
                stopifnot(is.list(result), "dimensions" %in% names(result))
                length(result$dimensions)
            }"""
            if operation == "metadata"
            else """function(result) {
                stopifnot(inherits(result, "Table"))
                c(result$num_rows, result$num_columns)
            }"""
        )
        self.r["gc"]()

    def run(self) -> Any:
        return self._run(self.path)

    def describe(self, result: Any) -> dict[str, int]:
        counts = self._describe(result)
        if self.operation == "metadata":
            return {"dimensions": int(counts[0])}
        return {"rows": int(counts[0]), "columns": int(counts[1])}
