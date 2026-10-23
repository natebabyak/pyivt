"""Generate reference Parquet using R packages cached only for this script."""

import os
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA_DIR = ROOT / "data"
EXPECTED_DIR = ROOT / "tests" / "expected"
R_LIBRARY = ROOT / ".cache" / "generate_expected_parquet"

R_SCRIPT = r"""
args <- commandArgs(trailingOnly = TRUE)
lib <- file.path(args[1], paste0("R-", R.version$major, ".",
                              strsplit(R.version$minor, ".", fixed = TRUE)[[1]][1]))
dir.create(lib, recursive = TRUE, showWarnings = FALSE)
Sys.setenv(R_LIBS_USER = lib, R_LIBS_SITE = lib)
.libPaths(c(lib, .Library), include.site = FALSE)
options(repos = c(CRAN = "https://cloud.r-project.org"), timeout = 300)

if (args[2] == "--install") {
    for (pkg in c("tzdb", "arrow", "remotes")) {
        if (!requireNamespace(pkg, quietly = TRUE)) {
            install.packages(pkg, lib = lib)
        }
        if (!requireNamespace(pkg, quietly = TRUE)) {
            stop("Could not install R package: ", pkg)
        }
    }
    if (!requireNamespace("canivt", quietly = TRUE)) {
        remotes::install_github("mountainMath/canivt", lib = lib,
                                dependencies = NA, upgrade = "never",
                                build = FALSE)
    }
    stopifnot(requireNamespace("canivt", quietly = TRUE))
} else {
    tab <- canivt::read_ivt(args[2])
    canivt::ivt_write_parquet(tab, args[3])
}
"""


def _rscript() -> str:
    if executable := shutil.which("Rscript"):
        return executable

    r_home = os.environ.get("R_HOME")
    if not r_home and os.name == "nt":
        import winreg

        for hive in (winreg.HKEY_CURRENT_USER, winreg.HKEY_LOCAL_MACHINE):
            try:
                with winreg.OpenKey(hive, r"Software\R-core\R") as key:
                    r_home = winreg.QueryValueEx(key, "InstallPath")[0]
                break
            except OSError:
                continue
    if r_home:
        executable = (
            Path(r_home) / "bin" / ("Rscript.exe" if os.name == "nt" else "Rscript")
        )
        if executable.is_file():
            return str(executable)
    raise RuntimeError("Install R and put Rscript on PATH, or set R_HOME.")


def main() -> int:
    ivt_paths = sorted(DATA_DIR.glob("*.ivt"))
    if not ivt_paths:
        return 0

    command = [_rscript(), "--vanilla", "-", str(R_LIBRARY)]
    print(f"Preparing isolated R library: {R_LIBRARY}", flush=True)
    subprocess.run([*command, "--install"], input=R_SCRIPT, text=True, check=True)
    EXPECTED_DIR.mkdir(parents=True, exist_ok=True)
    for ivt_path in ivt_paths:
        parquet_path = EXPECTED_DIR / ivt_path.with_suffix(".parquet").name
        print(f"Writing {parquet_path.name}", flush=True)
        subprocess.run(
            [*command, str(ivt_path), str(parquet_path)],
            input=R_SCRIPT,
            text=True,
            check=True,
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
