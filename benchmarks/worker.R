# Private worker: stdout contains only the benchmark line protocol.
args <- commandArgs(trailingOnly = TRUE)
operation <- args[[1]]
source <- args[[2]]
if (length(args) >= 3 && nzchar(args[[3]])) {
    root <- args[[3]]
    version <- paste0("R-", R.version$major, ".",
                      strsplit(R.version$minor, ".", fixed = TRUE)[[1]][1])
    .libPaths(c(file.path(root, version), root, .libPaths()))
}
for (package in c("canivt", if (operation == "arrow") "arrow")) {
    if (!requireNamespace(package, quietly = TRUE)) {
        cat(sprintf('{"unavailable":"R package %s is not installed"}\n', package))
        flush.console()
        quit(status = 2)
    }
}
run <- if (operation == "metadata") {
    function() canivt::ivt_metadata(source)
} else {
    function() {
        ivt <- canivt::read_ivt(source, missing = TRUE, complete = TRUE)
        arrow::Table$create(canivt::ivt_tidy(ivt, missing = TRUE))
    }
}
invisible(gc())
cat(sprintf(
    '{"ready":true,"pid":%s,"backend_version":"%s","runtime":"%s","arrow_version":"%s"}\n',
    Sys.getpid(), as.character(utils::packageVersion("canivt")), as.character(getRversion()),
    if (operation == "arrow") as.character(utils::packageVersion("arrow")) else ""
))
flush.console()
input <- file("stdin", open = "r")
if (!length(readLines(input, n = 1, warn = FALSE))) quit(status = 0)
start <- proc.time()
result <- run()
elapsed <- proc.time() - start
output <- if (operation == "arrow") {
    stopifnot(inherits(result, "Table"))
    sprintf(',"rows":%s,"columns":%s', result$num_rows, result$num_columns)
} else {
    stopifnot(is.list(result), "dimensions" %in% names(result))
    sprintf(',"dimensions":%s', length(result$dimensions))
}
cat(sprintf('{"wall_seconds":%.9f,"cpu_seconds":%.9f%s}\n',
            elapsed[["elapsed"]], elapsed[["user.self"]] + elapsed[["sys.self"]], output))
flush.console()
# Retain the table until the monitor has sampled it.
invisible(readLines(input, n = 1, warn = FALSE))
