#!/usr/bin/env bash
# © 2020, ETH Zurich
#
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

for in_example in ../collected_code/*.rs;
do
    printf ${in_example}\\n
    RUST_BACKTRACE=1 make LOG_LEVEL=info,prusti_viper=trace RUN_FILE=${in_example} build run
    cp nll-facts/main/outlive_graph.dot ${in_example}_refcopy_outlive_graph.dot
done
