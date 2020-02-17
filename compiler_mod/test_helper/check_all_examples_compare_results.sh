#!/usr/bin/env bash
for in_ex in ../collected_code/*.rs;
do
    printf $in_ex\\n; in_ex_name=$(basename -- $in_ex)
    RUST_BACKTRACE=1 make LOG_LEVEL=info,prusti_viper=trace RUN_FILE=${in_ex} build run &> /dev/null


    diff -s nll-facts/main/error_graph_improved.dot ~/Dokumente/bsc_thesis_tryouts/copied_graphs/${in_ex_name}_error_graph_improved.dot
    if [[ $? -ne 0 ]]
      then
        bold=$(tput bold)
        normal=$(tput sgr0)
        printf "${bold}ERROR: printed improved error path graph for ${in_ex_name} changed!${normal}\n"
    fi

    diff -s nll-facts/error_graph.json ~/Dokumente/bsc_thesis_tryouts/copied_graphs/${in_ex_name}_error_graph.json
    if [[ $? -ne 0 ]]
      then
        bold=$(tput bold)
        normal=$(tput sgr0)
        printf "${bold}ERROR: dumped JSON of the error path graph for ${in_ex_name} changed!${normal}\n"
    fi
done
