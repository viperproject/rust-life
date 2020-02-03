#!/usr/bin/env bash
# © 2020, ETH Zurich
#
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

for in_ex in ../collected_code/*.rs;
do
    printf $in_ex\\n; in_ex_name=$(basename -- $in_ex)
    RUST_BACKTRACE=1 make LOG_LEVEL=info,prusti_viper=trace RUN_FILE=${in_ex} build run
    cp nll-facts/main/error_graph_improved.dot ~/Dokumente/bsc_thesis_tryouts/copied_graphs/${in_ex_name}_error_graph_improved.dot
    cp nll-facts/error_graph.json ~/Dokumente/bsc_thesis_tryouts/copied_graphs/${in_ex_name}_error_graph.json
done
