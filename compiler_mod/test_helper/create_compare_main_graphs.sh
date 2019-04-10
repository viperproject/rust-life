#!/usr/bin/env bash
for in_example in ../collected_code/*.rs;
do
    printf ${in_example}\\n
    RUST_BACKTRACE=1 make LOG_LEVEL=info,prusti_viper=trace RUN_FILE=${in_example} build run &> /dev/null
    # cp nll-facts/main/outlive_graph.dot ${in_example}_refcopy_outlive_graph.dot
    diff -s nll-facts/main/outlive_graph.dot ${in_example}_refcopy_outlive_graph.dot
    if [[ $? -ne 0 ]]
      then
        bold=$(tput bold)
        normal=$(tput sgr0)
        printf "${bold}ERROR: output graph for ${in_example} changed!${normal}\n"
    fi

done
