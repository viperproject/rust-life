// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

/// Code for finding `rustc::ty::sty::RegionVid` associated with local
/// reference typed variables.

use facts;
use dump_borrowck_info::regex::Regex;
use dump_borrowck_info::rustc::mir;
use dump_borrowck_info::rustc_data_structures::indexed_vec::Idx;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;

pub fn load_variable_regions(path: &Path) -> io::Result<HashMap<mir::Local, facts::Region>> {
    trace!("[enter] load_variable_regions(path={:?})", path);
    let mut variable_regions = HashMap::new();
    let file = File::open(path)?;
    let fn_sig = Regex::new(r"^fn [a-zA-Z\d_]+\((?P<args>.*)\) -> (?P<result>.*)\{$").unwrap();
    let arg = Regex::new(r"^_(?P<local>\d+): &'(?P<rvid>\d+)rv (mut)? [a-zA-Z\d_]+\s*$").unwrap();
    let local = Regex::new(r"^\s+let mut _(?P<local>\d+): &'(?P<rvid>\d+)rv ").unwrap();
    let scope_local = Regex::new(r"").unwrap();
    for line in io::BufReader::new(file).lines() {
        let line = line?;
        if let Some(caps) = fn_sig.captures(&line) {
            debug!("args: {} result: {}", &caps["args"], &caps["result"]);
            for arg_str in (&caps["args"]).split(", ") {
                if let Some(arg_caps) = arg.captures(arg_str) {
                    debug!("arg {} rvid {}", &arg_caps["local"], &arg_caps["rvid"]);
                    let local: usize = (&arg_caps["local"]).parse().unwrap();
                    let rvid: usize = (&arg_caps["rvid"]).parse().unwrap();
                    variable_regions.insert(mir::Local::new(local), rvid.into());
                }
            }
        }
        if let Some(local_caps) = local.captures(&line) {
            debug!("local {} rvid {}", &local_caps["local"], &local_caps["rvid"]);
            let local: usize = (&local_caps["local"]).parse().unwrap();
            let rvid: usize = (&local_caps["rvid"]).parse().unwrap();
            variable_regions.insert(mir::Local::new(local), rvid.into());
        }
        if let Some(scope_local_caps) = scope_local.captures(&line) {
            debug!("local {} rvid {}", &scope_local_caps["local"], &scope_local_caps["rvid"]);
            let local: usize = (&scope_local_caps["local"]).parse().unwrap();
            let rvid: usize = (&scope_local_caps["rvid"]).parse().unwrap();
            variable_regions.insert(mir::Local::new(local), rvid.into());
        }
    }
    trace!("[exit] load_variable_regions");
    Ok(variable_regions)
}
