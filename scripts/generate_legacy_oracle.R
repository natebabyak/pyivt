# Generate independent references from a caller-selected canivt checkout.
# Rscript scripts/generate_legacy_oracle.R CANIVT_ROOT INPUT_DIR OUTPUT_DIR R_LIBRARY
args <- commandArgs(trailingOnly = TRUE)
stopifnot(length(args) %in% c(3L, 4L))
if (length(args) == 4L) .libPaths(c(normalizePath(args[4L]), .libPaths()))
if (.Platform$OS.type == "windows") Sys.setlocale("LC_CTYPE", "English_Canada.utf8")
for (f in list.files(file.path(args[1L], "R"), full.names = TRUE)) source(f)
dir.create(args[3L], recursive = TRUE, showWarnings = FALSE)
for (f in list.files(args[2L], pattern = "[.]ivt$", full.names = TRUE, ignore.case = TRUE)) {
  warnings <- list()
  tab <- withCallingHandlers(read_ivt(f), warning = function(w) {
    warnings[[length(warnings) + 1L]] <<- list(class = class(w), message = conditionMessage(w))
    invokeRestart("muffleWarning")
  })
  name <- tools::file_path_sans_ext(basename(f))
  metadata <- list(metadata = tab$metadata, cell_status = tab$cell_status, warnings = warnings)
  jsonlite::write_json(metadata, file.path(args[3L], paste0(name, ".json")),
                       auto_unbox = TRUE, pretty = TRUE, null = "null", na = "null")
  con <- gzfile(file.path(args[3L], paste0(name, ".csv.gz")), "wt", encoding = "UTF-8")
  write.csv(tab$cells, con, row.names = FALSE, na = "")
  close(con)
  cat(name, nrow(tab$cells), sum(tab$cells$value, na.rm = TRUE), "\n")
}
