#!/usr/bin/env bash
for in_example in ../collected_code/*.rs;
do
    printf ${in_example}\\n
    RUST_BACKTRACE=1 make LOG_LEVEL=info,prusti_viper=trace RUN_FILE=${in_example} build run
    cp nll-facts/main/outlive_graph.dot ${in_example}_refcopy_outlive_graph.dot
done
